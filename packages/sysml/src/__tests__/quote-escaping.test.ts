import { describe, it, expect } from "vitest";
import { serializeToSysml } from "../sysml-serializer.js";
import type { SysmlElement } from "@sysml-bridge/model";

// Grammar rule (docs/sysml-v2-reference/grammar/SysMLv2Lexer.g4:893):
//   '\'' ('\\' . | ~['\\])* '\''
// Inside a quoted name, a raw ' or \ is illegal — both must be backslash-escaped.

function el(id: string, name: string, type = "PartDefinition"): SysmlElement {
  return {
    id,
    elementId: id,
    type,
    name,
    shortName: null,
    qualifiedName: null,
    ownerId: null,
    ownedElementIds: [],
    raw: { "@type": type },
  };
}

describe("quoted-name escaping", () => {
  it("escapes single quotes in element names", () => {
    const out = serializeToSysml([el("1", "Operator's Console")], []);
    expect(out).toContain("'Operator\\'s Console'");
    expect(out).not.toContain("'Operator's Console'");
  });

  it("escapes backslashes and control characters", () => {
    const out = serializeToSysml([el("2", "A\\B\nC")], []);
    expect(out).toContain("'A\\\\B\\nC'");
  });
});
