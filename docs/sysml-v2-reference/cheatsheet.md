# SysML v2 Trace / Verify / Usage Cheatsheet

Distilled patterns for traceability work. **Every ` ```sysml ` block below is
valid** and has been verified against the vendored grammar
(`grammar/SysMLv2Parser.g4`, `grammar/SysMLv2Lexer.g4`) with the local
validator — see `G1-cheatsheet-validation.txt` in the closure evidence. Blocks
shown as ` ```text ` are deliberately **invalid** counter-examples and are not
meant to parse.

Grammar rule names below refer to productions in `grammar/SysMLv2Parser.g4`.

---

## 1. Definition vs Usage (the core rule)

A **definition** (`requirement def`, `part def`, `action def`, …) declares a
type. A **usage** (`requirement r : R;`, `part p : P;`) is a **Feature** — an
occurrence of that type. Trace relationships connect Features, not types.

> **Rule:** the operands of `satisfy … by …`, `allocate … to …`, and `verify …`
> MUST be **usages** (Features), never **definitions**. This follows from the
> grammar: `satisfyRequirementUsage` consumes an `ownedReferenceSubsetting`
> (a reference to a Feature) and its `BY satisfactionSubjectMember` resolves to
> a `featureChainMember`; `allocationUsageDeclaration` (rule
> `allocationUsageDeclaration : ALLOCATE connectorPart`) connects features via
> `connectorPart`. Neither accepts a bare definition.

```sysml
package DefVsUsage {
    requirement def R;                 // DEFINITION (a type)
    requirement <'ANGARS-4'> r : R;    // USAGE (a Feature, short-name 'ANGARS-4')

    part def Sub;
    part sub : Sub;                    // USAGE

    // satisfy / allocate / verify operands must be USAGES, never the *def*:
    satisfy r by sub;                  // r and sub are both usages -> legal
}
```

---

## 2. `satisfy <reqUsage> by <featureUsage>;` (package level, usages only)

Grammar: `satisfyRequirementUsage`. The thing satisfied and the thing that
satisfies it are both Features.

```sysml
package SatisfyPattern {
    requirement def R1;
    part def Component;

    requirement r1 : R1;          // usage
    part component : Component;    // usage

    satisfy r1 by component;      // satisfyRequirementUsage: usages only
}
```

---

## 3. `allocate <usageA> to <usageB>;`

Grammar: `allocationUsage` / `allocationUsageDeclaration`. Allocate a behavioral
usage onto a structural usage (or any feature-to-feature allocation).

```sysml
package AllocatePattern {
    action def Manage;
    part def Controller;

    action manage : Manage;        // usage
    part controller : Controller;  // usage

    allocate manage to controller; // allocationUsageDeclaration
}
```

---

## 4. `dependency from <X> to <Y>;` (used for derive: Req -> Need)

Grammar: `dependency`
(`DEPENDENCY (identification? FROM)? qualifiedName ... TO qualifiedName ...`).
We model **derive** (a requirement deriving from a stakeholder Need) as a
dependency from the requirement to the need.

```sysml
package DependencyPattern {
    requirement def NeedDef;
    requirement def ReqDef;

    requirement need : NeedDef;    // stakeholder Need (usage)
    requirement req  : ReqDef;     // derived requirement (usage)

    dependency from req to need;   // derive: Req -> Need
}
```

---

## 5. `verify` placement — the resolved answer

`verify` is **only** legal as a `requirementVerificationMember` inside a
`requirementBody`. The canonical, validated form puts it inside the
`objective { … }` of a `verification def`:

```sysml
package VerifyPlacement {
    requirement def R1;
    part def Subsystem;

    requirement r1 : R1;          // usage
    part subsystem : Subsystem;

    verification def V {
        subject s : Subsystem;
        objective {
            verify r1;            // requirementVerificationMember
        }
    }
}
```

### Why this is the only legal placement (grammar chain)

- `verification def V { … }` body is a **`caseBody`**
  (`verificationCaseDefinition : … VERIFICATION DEF definitionDeclaration caseBody`).
- A `caseBody` may contain an **`objectiveMember`**
  (`caseBodyItem : … | objectiveMember`).
- `objectiveMember : memberPrefix OBJECTIVE objectiveRequirementUsage`, and
  `objectiveRequirementUsage : usageExtensionKeyword* constraintUsageDeclaration requirementBody`
  — so the `objective { … }` braces are a **`requirementBody`**.
- `requirementBody` may contain a **`requirementVerificationMember`**
  (`requirementBodyItem : … | requirementVerificationMember`), defined as
  `requirementVerificationMember : memberPrefix VERIFY requirementVerificationUsage`.

So `verify` lives at exactly one place: as a member of a `requirementBody`,
and the only `requirementBody` available inside a `verification def` is the one
opened by `objective`.

### Both of these are INVALID (do NOT emit them)

Top-level `verify X by Y;` — there is no top-level `verify` production; Cameo
reports `extraneous input 'verify'` (reproduced by the local validator):

```text
package P {
    requirement def R1;
    verification def V1;
}
verify R1 by V1;          // INVALID: "extraneous input 'verify'"
```

`verify` placed directly in a `verification def {}` body with no enclosing
`objective` — the `verification def` body is a `caseBody`, which does NOT list
`requirementVerificationMember`, so again `extraneous input 'verify'`:

```text
package P {
    requirement def R1;
    requirement r1 : R1;
    verification def V1 {
        subject s;
        verify r1;        // INVALID: needs an enclosing objective { ... }
    }
}
```

---

## 6. Complete minimal valid example

A package with requirements as usages, a part def + part usage, an action
usage, `satisfy` / `allocate` / `dependency`, and a `verification def` with an
`objective { verify … }`:

```sysml
package CC {
    // --- definitions ---
    part def Subsystem;
    part def Controller;
    requirement def R1;
    action def Manage;

    // --- usages (trace participants must be usages / Features) ---
    part ccSubsystem : Subsystem {
        part controller : Controller;
    }
    requirement r1 : R1;
    action manage : Manage;

    // --- traceability at package / usage level (usages only) ---
    satisfy r1 by controller;       // satisfyRequirementUsage
    allocate manage to controller;  // allocationUsage
    dependency from r1 to manage;   // derive / trace via dependency

    // --- correct verify placement: inside objective of a verification def ---
    verification def VerifyR1 {
        subject ccSubsystem;
        objective {
            verify r1;
        }
    }
}
```
