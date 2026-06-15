import { z } from "zod";

export const SCHEMA_VERSION = "1.0.0" as const;

const NeedSchema = z.object({
  id: z.string(),
  kind: z.literal("need"),
  naturalKey: z.string(),
  name: z.string(),
  category: z.string().optional(),
  description: z.string().optional(),
});

const RequirementSchema = z.object({
  id: z.string(),
  kind: z.literal("requirement"),
  naturalKey: z.string(),
  name: z.string(),
  statement: z.string(),
  needIds: z.array(z.string()),
  verifyMethod: z.string().optional(),
  category: z.string().optional(),
  reqType: z.string().optional(),
});

// ---- Phase-2 provenance and seam sub-schemas ----

const ProvenanceSchema = z.object({
  workbook: z.string(),
  sheet: z.string(),
  row: z.number().int().optional(),
  cell: z.string().optional(),
});

const SubsystemSchema = z.object({
  id: z.string(),
  kind: z.literal("subsystem"),
  naturalKey: z.string(),
  name: z.string(),
  componentIds: z.array(z.string()),
  provenance: ProvenanceSchema,
});

const N2TripleSchema = z.object({
  id: z.string(),
  kind: z.literal("n2"),
  scope: z.enum(["subsystem", "component", "functional", "external"]),
  sourceId: z.string(),
  targetId: z.string(),
  sourceLabel: z.string(),
  targetLabel: z.string(),
  flow: z.string(),
  provenance: ProvenanceSchema,
});

const KppSchema = z.object({
  id: z.string(),
  kind: z.literal("kpp"),
  naturalKey: z.string(),
  title: z.string(),
  reqId: z.string().optional(),
  provenance: ProvenanceSchema,
});

const BehaviorDecompSchema = z.object({
  id: z.string(),
  kind: z.literal("behaviorDecomp"),
  naturalKey: z.string(),
  parentId: z.string().optional(),
  level: z.string(),
  name: z.string(),
  owner: z.string().optional(),
  provenance: ProvenanceSchema,
});

const FunctionSchema = z.object({
  id: z.string(),
  kind: z.literal("function"),
  naturalKey: z.string(),
  name: z.string(),
  level: z.string(),
  owner: z.string(),
});

const ComponentSchema = z.object({
  id: z.string(),
  kind: z.literal("component"),
  naturalKey: z.string(),
  name: z.string(),
});

const SatisfiesSchema = z.object({
  reqId: z.string(),
  functionId: z.string(),
});

const AllocationSchema = z.object({
  functionId: z.string(),
  componentId: z.string(),
});

export const ExtractedSchema = z.object({
  schema_version: z.literal(SCHEMA_VERSION),
  subsystem: z.string(),
  needs: z.array(NeedSchema),
  requirements: z.array(RequirementSchema),
  functions: z.array(FunctionSchema),
  components: z.array(ComponentSchema),
  satisfies: z.array(SatisfiesSchema),
  allocations: z.array(AllocationSchema),
  // Phase-2 extension seam — optional arrays so Phase-1 files continue to validate
  // and Phase 2 can extend without a major version bump or a god-object:
  subsystems: z.array(SubsystemSchema).optional(),
  n2Interfaces: z.array(N2TripleSchema).optional(),
  kpps: z.array(KppSchema).optional(),
  behaviorDecomp: z.array(BehaviorDecompSchema).optional(),
});

export type Extracted = z.infer<typeof ExtractedSchema>;
