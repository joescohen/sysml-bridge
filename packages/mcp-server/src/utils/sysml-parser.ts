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

/**
 * Strip leading block comments (/* ... *\/) and line comments (//) from text.
 * Returns the cleaned text.
 */
function stripComments(text: string): string {
  // Remove block comments (non-greedy)
  text = text.replace(/\/\*[\s\S]*?\*\//g, "");
  // Remove line comments
  text = text.replace(/\/\/[^\n]*/g, "");
  return text;
}

/**
 * Given the rest of a line after the keyword, extract:
 *   - optional shortName from <'...'> or <"...">
 *   - the element name (identifier or quoted)
 *   - optional typedBy from `: TypeName` (but NOT `:>`)
 *   - optional specializes from `:> TypeName`
 */
/**
 * Read a SysML v2 quoted name beginning at `s[0] === "'"`, honoring the grammar
 * escape rules (`\'` → `'`, `\\` → `\`; see SysMLv2Lexer.g4 STRING token). Returns
 * the unescaped value and the remaining text after the closing quote, or null if
 * the name is unterminated. A naive `indexOf("'", 1)` mis-stops on an escaped
 * quote, so a serialized name like `'O\'Brien'` must be read this way to round-trip.
 */
function readQuotedName(s: string): { value: string; rest: string } | null {
  let i = 1;
  let out = "";
  while (i < s.length) {
    const c = s[i];
    if (c === "\\" && i + 1 < s.length) {
      out += s[i + 1];
      i += 2;
      continue;
    }
    if (c === "'") {
      return { value: out, rest: s.slice(i + 1) };
    }
    out += c;
    i++;
  }
  return null;
}

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
    const q = readQuotedName(remaining);
    if (q === null) return null;
    name = q.value;
    remaining = q.rest.trim();
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

  // verify <req> by <element>;
  const verifyMatch = line.match(/^verify\s+([a-zA-Z_][a-zA-Z0-9_.]*)\s+by\s+([a-zA-Z_][a-zA-Z0-9_.]*)/);
  if (verifyMatch) {
    return { type: "verify", requirement: verifyMatch[1], by: verifyMatch[2] };
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
 * Match a bare, nested `verify <ref>;` statement — the form the serializer
 * emits inside a verification def's `objective { ... }` body. This is distinct
 * from the flat `verify <req> by <element>;` relationship (handled in
 * matchRelationship); the bare form has no `by` clause, so its source is the
 * enclosing verification case (resolved by the caller from the element stack).
 *
 * Returns the (unquoted) requirement reference, or null if the line is not a
 * bare verify. The ref may be a dotted/qualified identifier or a quoted name.
 */
function matchBareVerify(line: string): string | null {
  const m = line.match(
    /^verify\s+(?:'([^']+)'|([a-zA-Z_][a-zA-Z0-9_.]*))\s*$/
  );
  if (!m) return null;
  return m[1] ?? m[2];
}

/**
 * Match a bare enumeration literal — the form the serializer emits for a child
 * of an EnumerationDefinition: `<name>;` with no leading keyword (e.g. the
 * `LiIon;` / `NiMH;` inside `enum def Chem { LiIon; NiMH; }`). The literal is a
 * lone identifier or quoted name on the line.
 *
 * Returns the (unquoted) literal name, or null if the line is not a bare name.
 * Only meaningful when the enclosing element is an EnumerationDefinition; the
 * caller enforces that context.
 */
function matchEnumLiteral(line: string): string | null {
  const m = line.match(/^(?:'([^']+)'|([a-zA-Z_][a-zA-Z0-9_]*))\s*$/);
  if (!m) return null;
  return m[1] ?? m[2];
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

/**
 * Walk the element-scope stack from the innermost level outward and return the
 * first real (non-null) element. Brace levels opened by non-element keywords
 * (e.g. `objective {`) push a null sentinel, so the nearest enclosing element
 * may be several levels up — for a nested `verify` this is the verification
 * case that owns the objective body.
 */
function nearestEnclosingElement(
  elementStack: (ParsedElement | null)[]
): ParsedElement | null {
  for (let i = elementStack.length - 1; i >= 0; i--) {
    const el = elementStack[i];
    if (el) return el;
  }
  return null;
}

// ---------------------------------------------------------------------------
// Main parse function
// ---------------------------------------------------------------------------

export function parseSysml(text: string): ParseResult {
  const relationships: ParsedRelationship[] = [];
  const imports: string[] = [];
  const errors: string[] = [];

  // Strip comments first
  const cleaned = stripComments(text);

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

  for (const rawLine of lines) {
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
        currentList.push(el);
        continue;
      }

      // --- Nested `verify <ref>;` (inside a verification def objective body) ---
      // The serializer emits verify edges as a nested objective body, not a
      // flat `verify .. by ..` line. Recover it as a verify relationship whose
      // source is the enclosing verification case (`by`) and whose target is
      // the referenced requirement (`requirement`).
      const verifyTarget = matchBareVerify(lineForParsing);
      if (verifyTarget !== null) {
        const enclosing = nearestEnclosingElement(elementStack);
        relationships.push({
          type: "verify",
          requirement: verifyTarget,
          ...(enclosing ? { by: enclosing.name } : {}),
        });
        continue;
      }

      // --- Bare enumeration literal (child of an EnumerationDefinition) ---
      // e.g. `LiIon;` inside `enum def Chem { ... }`. Recovered as an
      // EnumerationUsage child of the enclosing enum def.
      const enclosingParent = elementStack[elementStack.length - 1];
      if (enclosingParent && enclosingParent.type === "EnumerationDefinition") {
        const literalName = matchEnumLiteral(lineForParsing);
        if (literalName !== null) {
          currentList.push({
            type: "EnumerationUsage",
            name: literalName,
            children: [],
            attributes: {},
          });
          continue;
        }
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
//
// Braces inside single-quoted names (which the serializer emits for names that
// are not valid identifiers, e.g. `part def 'Has{Brace}';`) MUST NOT be treated
// as nesting delimiters — otherwise brace-depth tracking is corrupted and the
// element identity/nesting is lost. We track quote state and ignore `{`/`}`
// while inside a quoted name. A backslash escapes the next character so an
// escaped quote (`\'`) does not toggle the quote state.
// ---------------------------------------------------------------------------

function splitOnBraces(line: string): string[] {
  const result: string[] = [];
  let current = "";
  let inQuote = false;
  for (let i = 0; i < line.length; i++) {
    const ch = line[i];
    // Copy an escaped pair (`\x`) verbatim so it never toggles quote state.
    if (ch === "\\" && i + 1 < line.length) {
      current += ch + line[i + 1];
      i++;
      continue;
    }
    if (ch === "'") {
      inQuote = !inQuote;
      current += ch;
      continue;
    }
    if (!inQuote && (ch === "{" || ch === "}")) {
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
