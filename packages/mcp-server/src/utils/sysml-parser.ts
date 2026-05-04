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
  children: ParsedElement[];
  attributes: Record<string, unknown>;
}

export interface ParsedRelationship {
  type: "satisfy" | "verify" | "allocate" | "dependency";
  requirement?: string;
  by?: string;
  from?: string;
  to?: string;
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
function parseElementRest(rest: string): {
  name: string;
  shortName?: string;
  typedBy?: string;
  specializes?: string;
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
  //    But first capture typing / specialization annotations.

  // Look for `:>` (specialization) — must come before `:` check
  let specializes: string | undefined;
  const specializesMatch = remaining.match(/:>\s*([a-zA-Z_][a-zA-Z0-9_:.]*)/);
  if (specializesMatch) {
    specializes = specializesMatch[1];
  }

  // Look for `:` that is NOT part of `:>` (typing)
  let typedBy: string | undefined;
  // Replace :> so we don't accidentally match it as ":"
  const withoutSpecializes = remaining.replace(/:>[^;{]*/g, "");
  const typedByMatch = withoutSpecializes.match(/:\s*([a-zA-Z_][a-zA-Z0-9_:.]*)/);
  if (typedByMatch) {
    typedBy = typedByMatch[1];
  }

  return { name, shortName, typedBy, specializes };
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
