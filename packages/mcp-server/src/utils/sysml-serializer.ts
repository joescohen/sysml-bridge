import type { SysmlElement, SysmlRelationship } from "../types/sysml-elements.js";

export function serializeToSysml(
  elements: SysmlElement[],
  relationships: SysmlRelationship[]
): string {
  const lines: string[] = [];

  const packages = elements.filter((e) => e.type === "Package");
  const nonPackages = elements.filter((e) => e.type !== "Package");

  for (const pkg of packages) {
    lines.push(`package ${escapeName(pkg.name)} {`);

    const children = nonPackages.filter(
      (e) => e.attributes?.owner?.["@id"] === pkg.id
    );
    for (const child of children) {
      lines.push(serializeElement(child, relationships, 1));
    }

    lines.push("}");
    lines.push("");
  }

  const orphans = nonPackages.filter(
    (e) =>
      !packages.some((p) => e.attributes?.owner?.["@id"] === p.id)
  );
  for (const orphan of orphans) {
    lines.push(serializeElement(orphan, relationships, 0));
  }

  return lines.join("\n");
}

function serializeElement(
  element: SysmlElement,
  relationships: SysmlRelationship[],
  indent: number
): string {
  const prefix = "  ".repeat(indent);
  const keyword = typeToKeyword(element.type);
  const relatedRels = relationships.filter(
    (r) => r.sourceId === element.id || r.targetId === element.id
  );

  let line = `${prefix}${keyword} ${escapeName(element.name)}`;

  if (relatedRels.length > 0) {
    line += " {";
    const relLines = relatedRels.map(
      (r) => `${prefix}  /* ${r.type}: ${r.sourceId} -> ${r.targetId} */`
    );
    return [line, ...relLines, `${prefix}}`].join("\n");
  }

  return `${line};`;
}

function typeToKeyword(type: string): string {
  const map: Record<string, string> = {
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
    AnalysisCaseDefinition: "analysis case def",
    AnalysisCaseUsage: "analysis case",
    VerificationCaseDefinition: "verification case def",
    VerificationCaseUsage: "verification case",
  };
  return map[type] ?? type;
}

function escapeName(name: string): string {
  if (/^[a-zA-Z_][a-zA-Z0-9_]*$/.test(name)) return name;
  return `'${name}'`;
}
