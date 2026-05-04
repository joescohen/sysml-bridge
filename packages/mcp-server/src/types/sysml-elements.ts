export interface SysmlElement {
  id: string;
  elementId: string;
  type: string;
  name: string | null;
  shortName: string | null;
  qualifiedName: string | null;
  ownerId: string | null;
  ownedElementIds: string[];
  raw: Record<string, unknown>;
}

export interface SysmlRelationship {
  id: string;
  type: string;
  sourceIds: string[];
  targetIds: string[];
  raw: Record<string, unknown>;
}

export interface ProjectState {
  projectId: string;
  commitId: string;
  branchId: string;
  totalElements: number;
  elementCountsByType: Record<string, number>;
}

export const SYSML_DEFINITION_TYPES = [
  "Package",
  "PartDefinition",
  "PortDefinition",
  "ConnectionDefinition",
  "InterfaceDefinition",
  "ItemDefinition",
  "AttributeDefinition",
  "RequirementDefinition",
  "ConstraintDefinition",
  "ActionDefinition",
  "StateDefinition",
  "UseCaseDefinition",
  "AllocationDefinition",
  "ViewDefinition",
  "ViewpointDefinition",
  "ConcernDefinition",
  "AnalysisCaseDefinition",
  "VerificationCaseDefinition",
  "EnumerationDefinition",
  "OccurrenceDefinition",
  "MetadataDefinition",
  "CalcDefinition",
  "RenderingDefinition",
] as const;

export const SYSML_USAGE_TYPES = [
  "PartUsage",
  "PortUsage",
  "ConnectionUsage",
  "InterfaceUsage",
  "ItemUsage",
  "AttributeUsage",
  "RequirementUsage",
  "ConstraintUsage",
  "ActionUsage",
  "StateUsage",
  "UseCaseUsage",
  "AllocationUsage",
  "ViewUsage",
  "ViewpointUsage",
  "AnalysisCaseUsage",
  "VerificationCaseUsage",
  "EnumerationUsage",
  "OccurrenceUsage",
  "CalcUsage",
  "RenderingUsage",
] as const;

export const SYSML_RELATIONSHIP_TYPES = [
  "OwningMembership",
  "FeatureMembership",
  "FeatureTyping",
  "Subsetting",
  "Redefinition",
  "Specialization",
  "Subclassification",
  "Conjugation",
  "Dependency",
  "Connector",
  "BindingConnector",
  "Annotation",
  "SatisfyRequirementUsage",
  "RequirementVerificationMembership",
] as const;

export type SysmlDefinitionType = (typeof SYSML_DEFINITION_TYPES)[number];
export type SysmlUsageType = (typeof SYSML_USAGE_TYPES)[number];
export type SysmlRelationshipType = (typeof SYSML_RELATIONSHIP_TYPES)[number];
