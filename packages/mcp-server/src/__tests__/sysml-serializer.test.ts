import { describe, it, expect } from "vitest";
import { serializeToSysml } from "../utils/sysml-serializer.js";
import type { SysmlElement, SysmlRelationship } from "../types/sysml-elements.js";

// ---------------------------------------------------------------------------
// Helper
// ---------------------------------------------------------------------------

function el(overrides: Partial<SysmlElement>): SysmlElement {
  return {
    id: "e1",
    elementId: "e1",
    type: "PartDefinition",
    name: "Test",
    shortName: null,
    qualifiedName: null,
    ownerId: null,
    ownedElementIds: [],
    raw: {},
    ...overrides,
  };
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

describe("serializeToSysml", () => {
  it("serializes a part definition", () => {
    const result = serializeToSysml(
      [el({ id: "e1", type: "PartDefinition", name: "Engine" })],
      []
    );
    expect(result).toBe("part def Engine;\n");
  });

  it("serializes a requirement definition", () => {
    const result = serializeToSysml(
      [el({ id: "e1", type: "RequirementDefinition", name: "MassReq" })],
      []
    );
    expect(result).toBe("requirement def MassReq;\n");
  });

  it("serializes a package with children", () => {
    const parent = el({ id: "pkg1", type: "Package", name: "SystemPkg", ownerId: null });
    const child = el({ id: "e2", type: "PartDefinition", name: "Engine", ownerId: "pkg1" });
    const result = serializeToSysml([parent, child], []);
    expect(result).toBe("package SystemPkg {\n  part def Engine;\n}\n");
  });

  it("serializes VerificationCaseDefinition with keyword 'verification def'", () => {
    const result = serializeToSysml(
      [el({ id: "e1", type: "VerificationCaseDefinition", name: "MassTest" })],
      []
    );
    expect(result).toBe("verification def MassTest;\n");
  });

  it("serializes AnalysisCaseDefinition with keyword 'analysis def'", () => {
    const result = serializeToSysml(
      [el({ id: "e1", type: "AnalysisCaseDefinition", name: "FuelAnalysis" })],
      []
    );
    expect(result).toBe("analysis def FuelAnalysis;\n");
  });

  it("serializes EnumerationDefinition with keyword 'enum def'", () => {
    const result = serializeToSysml(
      [el({ id: "e1", type: "EnumerationDefinition", name: "FuelKind" })],
      []
    );
    expect(result).toBe("enum def FuelKind;\n");
  });

  it("serializes elements with short names", () => {
    const result = serializeToSysml(
      [el({ id: "e1", type: "RequirementDefinition", name: "MaxMass", shortName: "SYS-001" })],
      []
    );
    expect(result).toBe("requirement def <'SYS-001'> MaxMass;\n");
  });

  it("quotes names that are not valid identifiers", () => {
    const result = serializeToSysml(
      [el({ id: "e1", type: "PartDefinition", name: "My Engine" })],
      []
    );
    expect(result).toBe("part def 'My Engine';\n");
  });

  it("does not quote plain valid identifier names", () => {
    const result = serializeToSysml(
      [el({ id: "e1", type: "PartDefinition", name: "Engine" })],
      []
    );
    expect(result).not.toContain("'Engine'");
    expect(result).toContain("Engine;");
  });

  it("treats elements whose ownerId is not in the element set as root", () => {
    // ownerId points to something outside the element array → treated as root
    const child = el({ id: "e1", type: "PartDefinition", name: "Engine", ownerId: "missing-parent" });
    const result = serializeToSysml([child], []);
    expect(result).toBe("part def Engine;\n");
  });

  it("handles deeply nested children", () => {
    const pkg = el({ id: "pkg1", type: "Package", name: "TopPkg", ownerId: null });
    const part = el({ id: "p1", type: "PartDefinition", name: "EngineDef", ownerId: "pkg1" });
    const port = el({ id: "port1", type: "PortDefinition", name: "FuelPort", ownerId: "p1" });
    const result = serializeToSysml([pkg, part, port], []);
    expect(result).toBe(
      "package TopPkg {\n  part def EngineDef {\n    port def FuelPort;\n  }\n}\n"
    );
  });

  it("handles a null name by omitting the name portion", () => {
    const result = serializeToSysml(
      [el({ id: "e1", type: "PartDefinition", name: null })],
      []
    );
    // Should produce: "part def;" (no name) — just verify it doesn't crash and has the keyword
    expect(result).toContain("part def");
  });

  it("outputs a trailing newline", () => {
    const result = serializeToSysml(
      [el({ id: "e1", type: "PartDefinition", name: "Engine" })],
      []
    );
    expect(result.endsWith("\n")).toBe(true);
  });

  it("emits a satisfy statement for SatisfyRequirementUsage", () => {
    const req = el({ id: "r1", type: "RequirementDefinition", name: "AircraftIDVerification" });
    const act = el({ id: "a1", type: "ActionDefinition", name: "ReceiveAuthenticateRequest" });
    const rel: SysmlRelationship = { id: "rel1", type: "SatisfyRequirementUsage", sourceIds: ["a1"], targetIds: ["r1"], raw: {} };
    expect(serializeToSysml([req, act], [rel])).toContain("satisfy AircraftIDVerification by ReceiveAuthenticateRequest;");
  });
  it("emits an allocate statement for AllocationUsage", () => {
    const fn = el({ id: "f1", type: "ActionDefinition", name: "ReceiveAuthenticateRequest" });
    const comp = el({ id: "c1", type: "PartDefinition", name: "FlightControlModule" });
    const rel: SysmlRelationship = { id: "rel2", type: "AllocationUsage", sourceIds: ["f1"], targetIds: ["c1"], raw: {} };
    expect(serializeToSysml([fn, comp], [rel])).toContain("allocate ReceiveAuthenticateRequest to FlightControlModule;");
  });
  it("emits verify as a nested objective body in the verification def (not top-level)", () => {
    const req = el({ id: "r1", type: "RequirementUsage", name: "AircraftIDVerification" });
    const ver = el({ id: "v1", type: "VerificationCaseDefinition", name: "AuthVerification" });
    const rel: SysmlRelationship = { id: "rel3", type: "VerifyRequirementUsage", sourceIds: ["v1"], targetIds: ["r1"], raw: {} };
    const out = serializeToSysml([req, ver], [rel]);

    // Nested form: verification def { objective { verify <reqUsage>; } }
    expect(out).toContain("verification def AuthVerification {");
    expect(out).toContain("objective {");
    expect(out).toContain("verify AircraftIDVerification;");

    // Negative: there must be NO top-level `verify ... by ...` line, and no
    // package-level `verify` (only the indented one inside objective {}).
    expect(out).not.toMatch(/^\s*verify .* by /m);
    expect(out).not.toMatch(/^verify /m);
  });
  it("emits a dependency statement for DeriveRequirementUsage (Need->Req)", () => {
    const need = el({ id: "n1", type: "RequirementDefinition", name: "N4_AuthenticationSecurity" });
    const req = el({ id: "r1", type: "RequirementDefinition", name: "AircraftIDVerification" });
    const rel: SysmlRelationship = { id: "rel4", type: "DeriveRequirementUsage", sourceIds: ["r1"], targetIds: ["n1"], raw: {} };
    expect(serializeToSysml([need, req], [rel])).toContain("dependency from AircraftIDVerification to N4_AuthenticationSecurity;");
  });
  it("skips relationships whose endpoints are not in the element set", () => {
    const req = el({ id: "r1", type: "RequirementDefinition", name: "AircraftIDVerification" });
    const rel: SysmlRelationship = { id: "rel5", type: "SatisfyRequirementUsage", sourceIds: ["missing"], targetIds: ["r1"], raw: {} };
    expect(serializeToSysml([req], [rel])).not.toContain("satisfy");
  });
  it("emits a provenance comment when an element carries a source id", () => {
    const req = el({ id: "r1", type: "RequirementDefinition", name: "AircraftIDVerification", raw: { provenanceSourceId: "ANGARS-4" } });
    expect(serializeToSysml([req], [])).toContain("// @source: ANGARS-4");
  });
});

// ---------------------------------------------------------------------------
// T2 — Control node emission tests (decide / fork / join / merge)
// ---------------------------------------------------------------------------

describe("T2 — control node emission (decide/fork/join/merge)", () => {
  it("emits 'decide <name>;' for DecisionNode inside an action def body", () => {
    const actionDef = el({ id: "a1", type: "ActionDefinition", name: "RefuelingProcess", ownerId: null });
    const decideNode = el({ id: "d1", type: "DecisionNode", name: "fuelCheck", ownerId: "a1" });
    const out = serializeToSysml([actionDef, decideNode], []);
    expect(out).toContain("action def RefuelingProcess {");
    expect(out).toContain("decide fuelCheck;");
  });

  it("emits 'fork <name>;' for ForkNode inside an action def body", () => {
    const actionDef = el({ id: "a1", type: "ActionDefinition", name: "RefuelingProcess", ownerId: null });
    const forkNode = el({ id: "f1", type: "ForkNode", name: "dispatch", ownerId: "a1" });
    const out = serializeToSysml([actionDef, forkNode], []);
    expect(out).toContain("fork dispatch;");
  });

  it("emits 'join <name>;' for JoinNode inside an action def body", () => {
    const actionDef = el({ id: "a1", type: "ActionDefinition", name: "RefuelingProcess", ownerId: null });
    const joinNode = el({ id: "j1", type: "JoinNode", name: "sync", ownerId: "a1" });
    const out = serializeToSysml([actionDef, joinNode], []);
    expect(out).toContain("join sync;");
  });

  it("emits 'merge <name>;' for MergeNode inside an action def body", () => {
    const actionDef = el({ id: "a1", type: "ActionDefinition", name: "RefuelingProcess", ownerId: null });
    const mergeNode = el({ id: "m1", type: "MergeNode", name: "done", ownerId: "a1" });
    const out = serializeToSysml([actionDef, mergeNode], []);
    expect(out).toContain("merge done;");
  });

  it("emits guarded succession 'first X if <guard> then Y;' for Succession with guard in raw", () => {
    // An action def with two actions and a guarded succession between them.
    const actionDef = el({ id: "a1", type: "ActionDefinition", name: "RefuelingProcess", ownerId: null });
    const src = el({ id: "s1", type: "ActionUsage", name: "fuelCheck", ownerId: "a1" });
    const tgt = el({ id: "t1", type: "ActionUsage", name: "Proceed", ownerId: "a1" });
    const rel: SysmlRelationship = {
      id: "rel1",
      type: "Succession",
      sourceIds: ["s1"],
      targetIds: ["t1"],
      raw: { guard: "fuelSufficient", ownerId: "a1" },
    };
    const out = serializeToSysml([actionDef, src, tgt], [rel]);
    expect(out).toContain("first fuelCheck if fuelSufficient then Proceed;");
  });

  it("emits plain succession 'first X then Y;' when no guard in raw", () => {
    const actionDef = el({ id: "a1", type: "ActionDefinition", name: "RefuelingProcess", ownerId: null });
    const src = el({ id: "s1", type: "ActionUsage", name: "Receive", ownerId: "a1" });
    const tgt = el({ id: "t1", type: "ActionUsage", name: "Validate", ownerId: "a1" });
    const rel: SysmlRelationship = {
      id: "rel1",
      type: "Succession",
      sourceIds: ["s1"],
      targetIds: ["t1"],
      raw: { ownerId: "a1" },
    };
    const out = serializeToSysml([actionDef, src, tgt], [rel]);
    expect(out).toContain("first Receive then Validate;");
    expect(out).not.toContain(" if ");
  });

  it("emits a state 'do <ref>;' member for a StateActionMembership rel (bare identifier)", () => {
    const stateDef = el({ id: "sd1", type: "StateDefinition", name: "Modes", ownerId: null });
    const mode = el({ id: "st1", type: "StateUsage", name: "greet", ownerId: "sd1" });
    const action = el({ id: "ac1", type: "ActionUsage", name: "monitorProximity", ownerId: "fn0" });
    const rel: SysmlRelationship = {
      id: "sam1",
      type: "StateActionMembership",
      sourceIds: ["st1"],
      targetIds: ["ac1"],
      raw: { doRef: "monitorProximity", ownerId: "sd1" },
    };
    const out = serializeToSysml([stateDef, mode, action], [rel]);
    expect(out).toContain("state greet {");
    expect(out).toContain("do monitorProximity;");
    // No spurious bare relationship-element declaration.
    expect(out).not.toContain("StateActionMembership;");
  });

  it("quotes a state 'do' ref whose name is not a valid identifier (spaces / '&')", () => {
    const stateDef = el({ id: "sd1", type: "StateDefinition", name: "Modes", ownerId: null });
    const mode = el({ id: "st1", type: "StateUsage", name: "Docking and Fueling", ownerId: "sd1" });
    const action = el({ id: "ac1", type: "ActionUsage", name: "Monitor Flow & Stability", ownerId: "fn0" });
    const rel: SysmlRelationship = {
      id: "sam2",
      type: "StateActionMembership",
      sourceIds: ["st1"],
      targetIds: ["ac1"],
      raw: { doRef: "Monitor Flow & Stability", ownerId: "sd1" },
    };
    const out = serializeToSysml([stateDef, mode, action], [rel]);
    // The '&' and spaces force quoting so the member parses (Gate-2 clean).
    expect(out).toContain("do 'Monitor Flow & Stability';");
  });

  it("falls back to the target element name when a StateActionMembership has no doRef", () => {
    const stateDef = el({ id: "sd1", type: "StateDefinition", name: "Modes", ownerId: null });
    const mode = el({ id: "st1", type: "StateUsage", name: "meet", ownerId: "sd1" });
    const action = el({ id: "ac1", type: "ActionUsage", name: "Authenticate Aircraft", ownerId: "fn0" });
    const rel: SysmlRelationship = {
      id: "sam3",
      type: "StateActionMembership",
      sourceIds: ["st1"],
      targetIds: ["ac1"],
      raw: { ownerId: "sd1" }, // no doRef → resolve from target name
    };
    const out = serializeToSysml([stateDef, mode, action], [rel]);
    expect(out).toContain("do 'Authenticate Aircraft';");
  });

  it("emits a full control-flow fixture matching the demo: decide+fork+join+merge+guarded", () => {
    // Mirror examples/demos/activity-control-flow.sysml structure (simplified subset).
    const actionDef = el({ id: "a0", type: "ActionDefinition", name: "Refueling Request Handling", ownerId: null });
    const receive = el({ id: "a1", type: "ActionUsage", name: "Receive Request", ownerId: "a0" });
    const validate = el({ id: "a2", type: "ActionUsage", name: "Validate Fuel Capacity", ownerId: "a0" });
    const decide = el({ id: "d1", type: "DecisionNode", name: "fuelCheck", ownerId: "a0" });
    const reject = el({ id: "a3", type: "ActionUsage", name: "Reject Request", ownerId: "a0" });
    const fork = el({ id: "f1", type: "ForkNode", name: "dispatch", ownerId: "a0" });
    const join = el({ id: "j1", type: "JoinNode", name: "sync", ownerId: "a0" });
    const merge = el({ id: "m1", type: "MergeNode", name: "done", ownerId: "a0" });

    // Plain succession: receive → validate
    const rel1: SysmlRelationship = { id: "r1", type: "Succession", sourceIds: ["a1"], targetIds: ["a2"], raw: { ownerId: "a0" } };
    // Plain succession: validate → decide
    const rel2: SysmlRelationship = { id: "r2", type: "Succession", sourceIds: ["a2"], targetIds: ["d1"], raw: { ownerId: "a0" } };
    // Guarded: decide --fuelLow--> reject
    const rel3: SysmlRelationship = { id: "r3", type: "Succession", sourceIds: ["d1"], targetIds: ["a3"], raw: { guard: "fuelLow", ownerId: "a0" } };
    // Guarded: decide --fuelSufficient--> fork
    const rel4: SysmlRelationship = { id: "r4", type: "Succession", sourceIds: ["d1"], targetIds: ["f1"], raw: { guard: "fuelSufficient", ownerId: "a0" } };
    // Plain: join → merge
    const rel5: SysmlRelationship = { id: "r5", type: "Succession", sourceIds: ["j1"], targetIds: ["m1"], raw: { ownerId: "a0" } };

    const elements = [actionDef, receive, validate, decide, reject, fork, join, merge];
    const rels = [rel1, rel2, rel3, rel4, rel5];
    const out = serializeToSysml(elements, rels);

    expect(out).toContain("action def 'Refueling Request Handling' {");
    expect(out).toContain("decide fuelCheck;");
    expect(out).toContain("fork dispatch;");
    expect(out).toContain("join sync;");
    expect(out).toContain("merge done;");
    expect(out).toContain("first 'Receive Request' then 'Validate Fuel Capacity';");
    expect(out).toContain("first fuelCheck if fuelLow then 'Reject Request';");
    expect(out).toContain("first fuelCheck if fuelSufficient then dispatch;");
  });
});
