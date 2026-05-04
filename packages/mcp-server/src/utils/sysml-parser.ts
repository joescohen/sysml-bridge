interface ParsedElement {
  type: string;
  name: string;
  attributes: Record<string, unknown>;
}

interface ParseResult {
  elements: ParsedElement[];
  errors: string[];
}

const KEYWORD_TO_TYPE: Record<string, string> = {
  "part def": "PartDefinition",
  part: "PartUsage",
  "port def": "PortDefinition",
  port: "PortUsage",
  "connection def": "ConnectionDefinition",
  connection: "ConnectionUsage",
  "interface def": "InterfaceDefinition",
  interface: "InterfaceUsage",
  "item def": "ItemDefinition",
  item: "ItemUsage",
  "attribute def": "AttributeDefinition",
  attribute: "AttributeUsage",
  "requirement def": "RequirementDefinition",
  requirement: "RequirementUsage",
  "constraint def": "ConstraintDefinition",
  constraint: "ConstraintUsage",
  "action def": "ActionDefinition",
  action: "ActionUsage",
  "state def": "StateDefinition",
  state: "StateUsage",
  "use case def": "UseCaseDefinition",
  "use case": "UseCaseUsage",
  "allocation def": "AllocationDefinition",
  allocation: "AllocationUsage",
  "view def": "ViewDefinition",
  view: "ViewUsage",
  "viewpoint def": "ViewpointDefinition",
  viewpoint: "ViewpointUsage",
  package: "Package",
  "analysis case def": "AnalysisCaseDefinition",
  "analysis case": "AnalysisCaseUsage",
  "verification case def": "VerificationCaseDefinition",
  "verification case": "VerificationCaseUsage",
};

const SORTED_KEYWORDS = Object.keys(KEYWORD_TO_TYPE).sort(
  (a, b) => b.length - a.length
);

export function parseSysml(text: string): ParseResult {
  const elements: ParsedElement[] = [];
  const errors: string[] = [];
  const lines = text.split("\n");

  for (let i = 0; i < lines.length; i++) {
    const line = lines[i].trim();
    if (!line || line.startsWith("//") || line.startsWith("/*") || line === "}" || line === "};") {
      continue;
    }

    const matched = matchKeyword(line);
    if (matched) {
      elements.push({
        type: matched.type,
        name: matched.name,
        attributes: {},
      });
    }
  }

  return { elements, errors };
}

function matchKeyword(
  line: string
): { type: string; name: string } | null {
  for (const keyword of SORTED_KEYWORDS) {
    if (line.startsWith(keyword + " ")) {
      const rest = line.slice(keyword.length + 1).trim();
      const name = extractName(rest);
      if (name) {
        return { type: KEYWORD_TO_TYPE[keyword], name };
      }
    }
  }
  return null;
}

function extractName(rest: string): string | null {
  if (rest.startsWith("'")) {
    const end = rest.indexOf("'", 1);
    if (end === -1) return null;
    return rest.slice(1, end);
  }

  const match = rest.match(/^([a-zA-Z_][a-zA-Z0-9_]*)/);
  return match ? match[1] : null;
}
