export interface SysmlElement {
  id: string;
  type: string;
  name: string;
  attributes: Record<string, unknown>;
}

export interface SysmlRelationship {
  id: string;
  type: string;
  sourceId: string;
  targetId: string;
}

export interface ProjectState {
  projectId: string;
  totalElements: number;
  elementCountsByType: Record<string, number>;
}

export const SYSML_ELEMENT_TYPES = [
  "Package",
  "PartDefinition",
  "PartUsage",
  "PortDefinition",
  "PortUsage",
  "ConnectionDefinition",
  "ConnectionUsage",
  "InterfaceDefinition",
  "InterfaceUsage",
  "ItemDefinition",
  "ItemUsage",
  "AttributeDefinition",
  "AttributeUsage",
  "RequirementDefinition",
  "RequirementUsage",
  "ConstraintDefinition",
  "ConstraintUsage",
  "ActionDefinition",
  "ActionUsage",
  "StateDefinition",
  "StateUsage",
  "UseCaseDefinition",
  "UseCaseUsage",
  "AllocationDefinition",
  "AllocationUsage",
  "ViewDefinition",
  "ViewUsage",
  "ViewpointDefinition",
  "ViewpointUsage",
  "AnalysisCaseDefinition",
  "AnalysisCaseUsage",
  "VerificationCaseDefinition",
  "VerificationCaseUsage",
] as const;

export type SysmlElementType = (typeof SYSML_ELEMENT_TYPES)[number];

export const SYSML_RELATIONSHIP_TYPES = [
  "Dependency",
  "Redefinition",
  "Subsetting",
  "FeatureTyping",
  "Specialization",
  "SatisfyRequirementUsage",
  "RequirementVerificationMembership",
  "AllocationUsage",
] as const;

export type SysmlRelationshipType = (typeof SYSML_RELATIONSHIP_TYPES)[number];
