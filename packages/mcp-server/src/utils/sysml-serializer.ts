import type { SysmlElement, SysmlRelationship } from "../types/sysml-elements.js";

// ---------------------------------------------------------------------------
// SysML v2 type → textual keyword mapping
// ---------------------------------------------------------------------------

const TYPE_TO_KEYWORD: Record<string, string> = {
  Package: "package",
  PartDefinition: "part def",
  PartUsage: "part",
  PortDefinition: "port def",
  PortUsage: "port",
  ConnectionDefinition: "connection def",
  ConnectionUsage: "connection",
  InterfaceDefinition: "interface def",
  InterfaceUsage: "interface",
  ItemDefinition: "item def",
  ItemUsage: "item",
  AttributeDefinition: "attribute def",
  AttributeUsage: "attribute",
  RequirementDefinition: "requirement def",
  RequirementUsage: "requirement",
  ConstraintDefinition: "constraint def",
  ConstraintUsage: "constraint",
  ActionDefinition: "action def",
  ActionUsage: "action",
  StateDefinition: "state def",
  StateUsage: "state",
  UseCaseDefinition: "use case def",
  UseCaseUsage: "use case",
  AllocationDefinition: "allocation def",
  AllocationUsage: "allocation",
  ViewDefinition: "view def",
  ViewUsage: "view",
  ViewpointDefinition: "viewpoint def",
  ViewpointUsage: "viewpoint",
  ConcernDefinition: "concern def",
  ConcernUsage: "concern",
  // CRITICAL: VerificationCaseDefinition → "verification def" (NOT "verification case def")
  VerificationCaseDefinition: "verification def",
  VerificationCaseUsage: "verification",
  // CRITICAL: AnalysisCaseDefinition → "analysis def" (NOT "analysis case def")
  AnalysisCaseDefinition: "analysis def",
  AnalysisCaseUsage: "analysis",
  EnumerationDefinition: "enum def",
  EnumerationUsage: "enum",
  CalcDefinition: "calc def",
  CalcUsage: "calc",
  RenderingDefinition: "rendering def",
  RenderingUsage: "rendering",
  OccurrenceDefinition: "occurrence def",
  OccurrenceUsage: "occurrence",
  MetadataDefinition: "metadata def",
};

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

export function serializeToSysml(
  elements: SysmlElement[],
  relationships: SysmlRelationship[]
): string {
  // Build a set of all element IDs for fast lookup
  const elementIds = new Set(elements.map((e) => e.id));

  // Build map of elements by ID
  const elementById = new Map<string, SysmlElement>(
    elements.map((e) => [e.id, e])
  );

  // Find root elements: those whose ownerId is null or not present in the element set
  const roots = elements.filter(
    (e) => e.ownerId === null || !elementIds.has(e.ownerId)
  );

  const lines: string[] = [];

  for (const root of roots) {
    serializeElement(root, elementById, 0, lines);
  }

  // Ensure output ends with a single newline
  let result = lines.join("\n");
  if (!result.endsWith("\n")) {
    result += "\n";
  }

  return result;
}

// ---------------------------------------------------------------------------
// Private helpers
// ---------------------------------------------------------------------------

function serializeElement(
  element: SysmlElement,
  elementById: Map<string, SysmlElement>,
  depth: number,
  lines: string[]
): void {
  const prefix = "  ".repeat(depth);
  const keyword = TYPE_TO_KEYWORD[element.type] ?? element.type;

  // Build the header: keyword + optional shortName + name
  let header = `${prefix}${keyword}`;

  if (element.shortName !== null) {
    header += ` <'${element.shortName}'>`;
  }

  if (element.name !== null) {
    const quotedName = isValidIdentifier(element.name)
      ? element.name
      : `'${element.name}'`;
    header += ` ${quotedName}`;
  }

  // Find children (elements whose ownerId matches this element's id)
  const children = [...elementById.values()].filter(
    (e) => e.ownerId === element.id
  );

  if (children.length > 0) {
    lines.push(`${header} {`);
    for (const child of children) {
      serializeElement(child, elementById, depth + 1, lines);
    }
    lines.push(`${prefix}}`);
  } else {
    lines.push(`${header};`);
  }
}

function isValidIdentifier(name: string): boolean {
  return /^[a-zA-Z_][a-zA-Z0-9_]*$/.test(name);
}
