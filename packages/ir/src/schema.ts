import { z } from "zod";

export const SCHEMA_VERSION = "1.0.0" as const;

const NeedSchema = z.object({
  id: z.string(),
  kind: z.literal("need"),
  naturalKey: z.string(),
  name: z.string(),
});

const RequirementSchema = z.object({
  id: z.string(),
  kind: z.literal("requirement"),
  naturalKey: z.string(),
  name: z.string(),
  statement: z.string(),
  needIds: z.array(z.string()),
  verifyMethod: z.string().optional(),
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
  // Phase-2 extension seam — add as optional arrays so Phase-1 files continue to validate
  // and Phase 2 can extend without a major version bump or a god-object:
  // subsystems: z.array(SubsystemSchema).optional(),
  // n2Interfaces: z.array(N2TripleSchema).optional(),
  // kpps: z.array(KppSchema).optional(),
  // behaviorDecomp: z.array(BehaviorDecompSchema).optional(),
});

export type Extracted = z.infer<typeof ExtractedSchema>;
