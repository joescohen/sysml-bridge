// ---------------------------------------------------------------------------
// SysML v2 Textual Notation Parser
//
// A lightweight, practical parser. It is NOT a full grammar implementation —
// it extracts element declarations, relationships, and imports from .sysml
// text using pattern matching.
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// Exported types
// ---------------------------------------------------------------------------

export interface ParsedElement {
  type: string;
  name: string;
  shortName?: string;
  typedBy?: string;
  specializes?: string;
  /** Redefinition base captured from `:>> base`. */
  redefines?: string;
  /** Multiplicity captured from `[mult]` (e.g. "4", "1..*", "*"). */
  multiplicity?: string;
  /**
   * Recovered stable identity, captured from the `// @id: <uuid>` trailing
   * comment convention emitted by sysml-serializer.ts (SerializeOptions.emitElementIds)
   * — see extractIdCommentsByLine() for the R1 rationale. Undefined when the
   * source text carries no such annotation (e.g. hand-authored .sysml).
   */
  elementId?: string;
  children: ParsedElement[];
  attributes: Record<string, unknown>;
}

export interface ParsedRelationship {
  type:
    | "satisfy"
    | "verify"
    | "allocate"
    | "dependency"
    | "connect"
    | "bind"
    | "succession"
    | "flow"
    | "transition";
  requirement?: string;
  by?: string;
  from?: string;
  to?: string;
  /** Connection name for `connection L connect a to b;`. */
  name?: string;
}

export interface ParseResult {
  elements: ParsedElement[];
  relationships: ParsedRelationship[];
  imports: string[];
  errors: string[];
}

// ---------------------------------------------------------------------------
// Keyword → SysML v2 type mapping
// NOTE: Use correct SysML v2 keyword spelling:
//   - "verification def" (not "verification case def")
//   - "analysis def"     (not "analysis case def")
//   - "enum def"         (not "enumeration def")
// ---------------------------------------------------------------------------

const KEYWORD_TO_TYPE: Record<string, string> = {
  // Multi-word defs (must be listed before single-word variants)
  "use case def": "UseCaseDefinition",
  "use case": "UseCaseUsage",
  "part def": "PartDefinition",
  "port def": "PortDefinition",
  "connection def": "ConnectionDefinition",
  "interface def": "InterfaceDefinition",
  "item def": "ItemDefinition",
  "attribute def": "AttributeDefinition",
  "requirement def": "RequirementDefinition",
  "constraint def": "ConstraintDefinition",
  "action def": "ActionDefinition",
  "state def": "StateDefinition",
  "allocation def": "AllocationDefinition",
  "view def": "ViewDefinition",
  "viewpoint def": "ViewpointDefinition",
  "concern def": "ConcernDefinition",
  "verification def": "VerificationCaseDefinition",
  "analysis def": "AnalysisCaseDefinition",
  "enum def": "EnumerationDefinition",
  "calc def": "CalcDefinition",
  "rendering def": "RenderingDefinition",
  "occurrence def": "OccurrenceDefinition",
  "metadata def": "MetadataDefinition",
  // Single-word defs
  package: "Package",
  part: "PartUsage",
  port: "PortUsage",
  connection: "ConnectionUsage",
  interface: "InterfaceUsage",
  item: "ItemUsage",
  attribute: "AttributeUsage",
  requirement: "RequirementUsage",
  constraint: "ConstraintUsage",
  action: "ActionUsage",
  state: "StateUsage",
  allocation: "AllocationUsage",
  view: "ViewUsage",
  viewpoint: "ViewpointUsage",
  concern: "ConcernUsage",
  verification: "VerificationCaseUsage",
  analysis: "AnalysisCaseUsage",
  enum: "EnumerationUsage",
  calc: "CalcUsage",
  rendering: "RenderingUsage",
  occurrence: "OccurrenceUsage",
};

// Sort longest-first so multi-word keywords match before single-word ones
// e.g. "part def" before "part", "use case def" before "use case" before nothing.
const SORTED_KEYWORDS = Object.keys(KEYWORD_TO_TYPE).sort(
  (a, b) => b.length - a.length
);

// ---------------------------------------------------------------------------
// Keywords whose lines should be silently skipped (no error, no element)
// ---------------------------------------------------------------------------

const SKIP_PREFIXES = new Set([
  "first",
  "then",
  "flow",
  "connect",
  "in",
  "out",
  "return",
  "redefines",
  "perform",
  "exhibit",
  "send",
  "accept",
  "transition",
  "filter",
  "expose",
  "render",
  "subject",
  "objective",
  "require",
  "stakeholder",
  "frame",
  "end",
  "alias",
  "succeed",
  "precondition",
  "assert",
  "doc",
  "actor",
  "include",
]);

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/** Remove block comments (/* ... *\/, non-greedy). */
function stripBlockComments(text: string): string {
  return text.replace(/\/\*[\s\S]*?\*\//g, "");
}

/** Remove line comments (// ...). */
function stripLineComments(text: string): string {
  return text.replace(/\/\/[^\n]*/g, "");
}

/**
 * Strip leading block comments (/* ... *\/) and line comments (//) from text.
 * Returns the cleaned text.
 */
function stripComments(text: string): string {
  return stripLineComments(stripBlockComments(text));
}

/**
 * Recover per-line `elementId` values from the `// @id: <uuid>` trailing
 * comment convention (R1 fallback mechanism — see sysml-serializer.ts
 * SerializeOptions.emitElementIds / "Milestone 1: identity round-trip").
 *
 * WHY A COMMENT, NOT A METADATA ANNOTATION: the existing InferenceProvenance
 * metatag precedent (`metadata InferenceProvenance about <name> { ... }`) was
 * considered first, but rejected as disproportionate for a single opaque id
 * per element — that form requires generalized name-correlated block parsing
 * (ambiguous when two elements share a name across scopes; this parser's
 * existing bare-brace handling for skippable keywords like `objective` also
 * loses nesting context for such blocks — confirmed empirically: parsing
 * InferenceProvenance's own emitted "about" blocks today produces
 * "Unparseable line" errors, since they are never actually round-tripped).
 * A trailing `// @id:` comment is positionally unambiguous (same line as the
 * declaration, no name correlation needed), requires no new grammar
 * construct (`//` starts a `REGULAR_COMMENT`, a token the lexer SKIPS per
 * docs/sysml-v2-reference/grammar/SysMLv2Lexer.g4:916), and reuses a
 * mechanism already proven grammar-legal in this exact codebase (the
 * `// @source: <id>` provenance suffix emitted by the same serializeElement()
 * header-line path already passes tools/sysml-validator/run.sh on real
 * ANGARS output). NOTE: some tools (e.g. Cameo textual sync) strip comments
 * on round-trip — an id carried this way would not survive THAT hop; that is
 * explicitly out of scope for Milestone 1 (file-native serialize→parse→import
 * only), tracked for the SysmlV2ApiStore / Cameo interop spike (Milestone 3).
 *
 * Operates on text where BLOCK comments are already stripped (so its line
 * indices line up 1:1 with the `cleaned` text the main parse loop iterates)
 * but LINE comments are not yet stripped — this function reads them.
 */
function extractIdCommentsByLine(textAfterBlockStrip: string): Map<number, string> {
  const ids = new Map<number, string>();
  const idRe = /\/\/.*?@id:\s*(\S+)/;
  const lines = textAfterBlockStrip.split("\n");
  for (let i = 0; i < lines.length; i++) {
    const m = lines[i].match(idRe);
    if (m) ids.set(i, m[1]);
  }
  return ids;
}

/**
 * Given the rest of a line after the keyword, extract:
 *   - optional shortName from <'...'> or <"...">
 *   - the element name (identifier or quoted)
 *   - optional typedBy from `: TypeName` (but NOT `:>`)
 *   - optional specializes from `:> TypeName`
 */
function parseElementRest(rest: string): {
  name: string;
  shortName?: string;
  typedBy?: string;
  specializes?: string;
  redefines?: string;
  multiplicity?: string;
  value?: string;
} | null {
  let remaining = rest.trim();

  // 1. Optional short name: <'SYS-001'> or <"SYS-001">
  let shortName: string | undefined;
  const shortNameMatch = remaining.match(/^<['"]([^'"]+)['"]>\s*/);
  if (shortNameMatch) {
    shortName = shortNameMatch[1];
    remaining = remaining.slice(shortNameMatch[0].length);
  }

  // 2. Element name — identifier or quoted string
  let name: string | null = null;
  if (remaining.startsWith("'")) {
    const end = remaining.indexOf("'", 1);
    if (end === -1) return null;
    name = remaining.slice(1, end);
    remaining = remaining.slice(end + 1).trim();
  } else {
    const m = remaining.match(/^([a-zA-Z_][a-zA-Z0-9_]*)/);
    if (!m) return null;
    name = m[1];
    remaining = remaining.slice(m[0].length).trim();
  }

  // 3. Remove trailing `;` or `{` and whatever follows
  //    But first capture typing / specialization / redefinition / multiplicity.

  // Look for `:>>` (redefinition) — must be checked before `:>` and `:`.
  let redefines: string | undefined;
  const redefinesMatch = remaining.match(/:>>\s*([a-zA-Z_][a-zA-Z0-9_:.]*)/);
  if (redefinesMatch) {
    redefines = redefinesMatch[1];
  }

  // Look for `:>` (specialization / subsetting) that is NOT `:>>`.
  // Strip any `:>>` occurrences first so we don't match their `:>` prefix.
  let specializes: string | undefined;
  const withoutRedefines = remaining.replace(/:>>[^;{]*/g, "");
  const specializesMatch = withoutRedefines.match(/:>\s*([a-zA-Z_][a-zA-Z0-9_:.]*)/);
  if (specializesMatch) {
    specializes = specializesMatch[1];
  }

  // Look for `:` that is NOT part of `:>` / `:>>` (typing)
  let typedBy: string | undefined;
  // Replace :>> and :> so we don't accidentally match them as ":"
  const withoutSpecializes = remaining
    .replace(/:>>[^;{]*/g, "")
    .replace(/:>[^;{]*/g, "");
  const typedByMatch = withoutSpecializes.match(/:\s*([a-zA-Z_][a-zA-Z0-9_:.]*)/);
  if (typedByMatch) {
    typedBy = typedByMatch[1];
  }

  // Multiplicity: `[mult]` (e.g. [4], [1..*], [*]). Capture the inner text.
  let multiplicity: string | undefined;
  const multMatch = remaining.match(/\[\s*([^\]]+?)\s*\]/);
  if (multMatch) {
    multiplicity = multMatch[1];
  }

  // Value: ` = <value>` (e.g. `attribute capacity = 100`). Capture the literal
  // up to the statement terminator / body open. Excludes `:>>`/`:>` (handled
  // above) since those use `:`/`>` not `=`.
  let value: string | undefined;
  const valueMatch = remaining.match(/=\s*([^;{]+?)\s*(?:[;{]|$)/);
  if (valueMatch) {
    value = valueMatch[1].trim();
  }

  return {
    name,
    shortName,
    typedBy,
    specializes,
    redefines,
    multiplicity,
    value,
  };
}

/**
 * Try to match a line against a known element keyword.
 * Returns the parsed element fields or null.
 */
function matchElement(line: string): {
  type: string;
  name: string;
  shortName?: string;
  typedBy?: string;
  specializes?: string;
  redefines?: string;
  multiplicity?: string;
  value?: string;
} | null {
  for (const keyword of SORTED_KEYWORDS) {
    // Keyword must be followed by a space (not just a prefix of another word)
    if (line.startsWith(keyword + " ") || line.startsWith(keyword + "\t")) {
      const rest = line.slice(keyword.length).trim();
      const parsed = parseElementRest(rest);
      if (parsed) {
        return { type: KEYWORD_TO_TYPE[keyword], ...parsed };
      }
    }
  }
  return null;
}

/**
 * Try to parse an import line.
 * Handles: `import X;` and `private import X;`
 */
function matchImport(line: string): string | null {
  // Strip optional "private " prefix
  let rest = line;
  if (rest.startsWith("private ")) {
    rest = rest.slice("private ".length).trim();
  }
  if (!rest.startsWith("import ")) return null;
  rest = rest.slice("import ".length).trim();
  // Remove trailing semicolon and/or block-open
  rest = rest.replace(/[;{].*$/, "").trim();
  if (!rest) return null;
  return rest;
}

/**
 * Try to parse a relationship line.
 */
function matchRelationship(line: string): ParsedRelationship | null {
  // satisfy <req> by <element>;
  const satisfyMatch = line.match(/^satisfy\s+([a-zA-Z_][a-zA-Z0-9_.]*)\s+by\s+([a-zA-Z_][a-zA-Z0-9_.]*)/);
  if (satisfyMatch) {
    return { type: "satisfy", requirement: satisfyMatch[1], by: satisfyMatch[2] };
  }

  // verify <req> by <element>;  (legacy top-level form — INVALID per R3 /
  // CLAUDE.md: Cameo reports "extraneous input 'verify'" for this shape. Kept
  // so any input already using it is still recognized, but never emitted by
  // this serializer.)
  const verifyMatch = line.match(/^verify\s+([a-zA-Z_][a-zA-Z0-9_.]*)\s+by\s+([a-zA-Z_][a-zA-Z0-9_.]*)/);
  if (verifyMatch) {
    return { type: "verify", requirement: verifyMatch[1], by: verifyMatch[2] };
  }

  // verify <req>;  (bare form — the ONLY grammar-legal placement per R3:
  // nested inside `objective { ... }` in a `verification def`; see
  // cheatsheet.md §5). This function has no scope context, so `from` (the
  // owning verification case's name) is left unset here — the main parse
  // loop resolves it from the current elementStack and fills it in.
  const bareVerifyMatch = line.match(/^verify\s+([a-zA-Z_][a-zA-Z0-9_.]*)\s*$/);
  if (bareVerifyMatch) {
    return { type: "verify", requirement: bareVerifyMatch[1] };
  }

  // allocate <from> to <to>;
  const allocateMatch = line.match(/^allocate\s+([a-zA-Z_][a-zA-Z0-9_.]*)\s+to\s+([a-zA-Z_][a-zA-Z0-9_.]*)/);
  if (allocateMatch) {
    return { type: "allocate", from: allocateMatch[1], to: allocateMatch[2] };
  }

  // dependency from <x> to <y>;
  const dependencyMatch = line.match(/^dependency\s+from\s+([a-zA-Z_][a-zA-Z0-9_.]*)\s+to\s+([a-zA-Z_][a-zA-Z0-9_.]*)/);
  if (dependencyMatch) {
    return { type: "dependency", from: dependencyMatch[1], to: dependencyMatch[2] };
  }

  // connection <name> connect <a> to <b>;  (named connection)
  const namedConnectMatch = line.match(
    /^connection\s+([a-zA-Z_][a-zA-Z0-9_]*)\s+connect\s+([a-zA-Z_][a-zA-Z0-9_.]*)\s+to\s+([a-zA-Z_][a-zA-Z0-9_.]*)/
  );
  if (namedConnectMatch) {
    return {
      type: "connect",
      name: namedConnectMatch[1],
      from: namedConnectMatch[2],
      to: namedConnectMatch[3],
    };
  }

  // connect <a> to <b>;
  const connectMatch = line.match(/^connect\s+([a-zA-Z_][a-zA-Z0-9_.]*)\s+to\s+([a-zA-Z_][a-zA-Z0-9_.]*)/);
  if (connectMatch) {
    return { type: "connect", from: connectMatch[1], to: connectMatch[2] };
  }

  // bind <a> = <b>;
  const bindMatch = line.match(/^bind\s+([a-zA-Z_][a-zA-Z0-9_.]*)\s*=\s*([a-zA-Z_][a-zA-Z0-9_.]*)/);
  if (bindMatch) {
    return { type: "bind", from: bindMatch[1], to: bindMatch[2] };
  }

  // transition first <s1> then <s2>;  (checked before the bare first..then)
  const transitionMatch = line.match(
    /^transition\s+first\s+([a-zA-Z_][a-zA-Z0-9_.]*)\s+then\s+([a-zA-Z_][a-zA-Z0-9_.]*)/
  );
  if (transitionMatch) {
    return { type: "transition", from: transitionMatch[1], to: transitionMatch[2] };
  }

  // first <a> then <b>;  (succession)
  const successionMatch = line.match(/^first\s+([a-zA-Z_][a-zA-Z0-9_.]*)\s+then\s+([a-zA-Z_][a-zA-Z0-9_.]*)/);
  if (successionMatch) {
    return { type: "succession", from: successionMatch[1], to: successionMatch[2] };
  }

  // flow from <a> to <b>;  (only the `from .. to ..` shape; `flow of ..` falls
  // through to the skip-prefix handling so it is not mis-parsed)
  const flowMatch = line.match(/^flow\s+from\s+([a-zA-Z_][a-zA-Z0-9_.]*)\s+to\s+([a-zA-Z_][a-zA-Z0-9_.]*)/);
  if (flowMatch) {
    return { type: "flow", from: flowMatch[1], to: flowMatch[2] };
  }

  return null;
}

/**
 * Check if a line starts with one of the known skip-prefixes
 * (followed by a space, tab, semicolon, or end of string).
 */
function isSkippableLine(line: string): boolean {
  for (const prefix of SKIP_PREFIXES) {
    if (
      line === prefix ||
      line.startsWith(prefix + " ") ||
      line.startsWith(prefix + "\t") ||
      line.startsWith(prefix + ";")
    ) {
      return true;
    }
  }
  return false;
}

// ---------------------------------------------------------------------------
// Main parse function
// ---------------------------------------------------------------------------

export function parseSysml(text: string): ParseResult {
  const relationships: ParsedRelationship[] = [];
  const imports: string[] = [];
  const errors: string[] = [];

  // Recover `// @id: <uuid>` annotations BEFORE line comments are stripped
  // (extractIdCommentsByLine reads them; see its doc comment for the R1
  // rationale). Block comments are stripped first so line indices line up
  // 1:1 with `cleaned` below.
  const textAfterBlockStrip = stripBlockComments(text);
  const idsByLine = extractIdCommentsByLine(textAfterBlockStrip);
  const cleaned = stripLineComments(textAfterBlockStrip);

  // We need to track brace nesting to assign children.
  // We process token by token: lines may contain `{` or `}` characters.
  // Strategy: split the cleaned text into "tokens" — either element lines or
  // brace characters — so nesting is clear.

  // Normalise line endings
  const lines = cleaned.split("\n");

  // Stack of element arrays. The bottom of the stack is the top-level list.
  // When we open a `{`, we push the current element's children array.
  const stack: ParsedElement[][] = [];
  const topLevelElements: ParsedElement[] = [];
  // The element most recently pushed (whose body we are now in)
  const elementStack: (ParsedElement | null)[] = [null]; // null = top-level scope

  let currentList = topLevelElements;

  for (let lineIndex = 0; lineIndex < lines.length; lineIndex++) {
    const rawLine = lines[lineIndex];
    // We may have multiple logical tokens on one physical line, e.g.:
    //   "part def Engine {"   — element + open brace
    //   "}"                   — close brace
    // We process by splitting on `{` and `}` boundaries.
    const parts = splitOnBraces(rawLine.trim());

    for (const part of parts) {
      const token = part.trim();

      if (!token) continue;

      if (token === "{") {
        // Push a new nesting level — the last element added to currentList
        // becomes the parent. If none, push a null sentinel.
        const lastEl = currentList.length > 0 ? currentList[currentList.length - 1] : null;
        stack.push(currentList);
        elementStack.push(lastEl);
        if (lastEl) {
          currentList = lastEl.children;
        }
        // If no element to nest into (bare `{`), keep the same list
        // but still push to stack so the matching `}` pops correctly.
        continue;
      }

      if (token === "}" || token === "};") {
        if (stack.length > 0) {
          currentList = stack.pop()!;
          elementStack.pop();
        }
        continue;
      }

      // Strip trailing semicolon for parsing (keep content before `{`)
      // e.g. "part def Engine {" → parse "part def Engine"
      const lineForParsing = token.replace(/;$/, "").trim();

      if (!lineForParsing) continue;

      // Skip blank-after-strip
      if (!lineForParsing) continue;

      // --- Imports ---
      const importPath = matchImport(lineForParsing);
      if (importPath !== null) {
        imports.push(importPath);
        continue;
      }

      // --- Relationships ---
      const rel = matchRelationship(lineForParsing);
      if (rel !== null) {
        if (rel.type === "verify" && rel.by === undefined) {
          // Bare `verify <req>;` form: resolve the owning verification case
          // from the nearest non-null entry in elementStack (walking up past
          // the null sentinel pushed for the `objective {` bare-brace scope —
          // see the "bare brace" note on the `{` handling below).
          for (let i = elementStack.length - 1; i >= 0; i--) {
            const owner = elementStack[i];
            if (owner) {
              rel.from = owner.name;
              break;
            }
          }
        }
        relationships.push(rel);
        continue;
      }

      // --- Elements ---
      const elMatch = matchElement(lineForParsing);
      if (elMatch !== null) {
        const el: ParsedElement = {
          type: elMatch.type,
          name: elMatch.name,
          children: [],
          attributes: {},
        };
        if (elMatch.shortName !== undefined) el.shortName = elMatch.shortName;
        if (elMatch.typedBy !== undefined) el.typedBy = elMatch.typedBy;
        if (elMatch.specializes !== undefined) el.specializes = elMatch.specializes;
        if (elMatch.redefines !== undefined) el.redefines = elMatch.redefines;
        if (elMatch.multiplicity !== undefined) el.multiplicity = elMatch.multiplicity;
        if (elMatch.value !== undefined) el.attributes.value = elMatch.value;
        const recoveredId = idsByLine.get(lineIndex);
        if (recoveredId !== undefined) el.elementId = recoveredId;
        currentList.push(el);
        continue;
      }

      // --- Known skippable lines ---
      if (isSkippableLine(lineForParsing)) {
        continue;
      }

      // --- Unknown / unparseable ---
      errors.push(`Unparseable line: ${token}`);
    }
  }

  return { elements: topLevelElements, relationships, imports, errors };
}

// ---------------------------------------------------------------------------
// Utility: split a line into sub-tokens on `{` and `}` characters,
// preserving the brace characters as their own tokens.
// e.g. "part def Engine {" → ["part def Engine ", "{", ""]
//      "part a; part b }" → ["part a; part b ", "}"]
// ---------------------------------------------------------------------------

function splitOnBraces(line: string): string[] {
  const result: string[] = [];
  let current = "";
  for (const ch of line) {
    if (ch === "{" || ch === "}") {
      result.push(current);
      result.push(ch);
      current = "";
    } else {
      current += ch;
    }
  }
  if (current) result.push(current);
  return result;
}
