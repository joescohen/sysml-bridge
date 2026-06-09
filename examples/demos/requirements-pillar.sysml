// PILLAR: Requirements (+ Verification) -- Aerial Refueling Control
// Cross-pillar links demonstrated:
//   derive   : each requirement -> the stakeholder Need 'Reliable Refueling Operations' (dependency)
//   satisfy  : R-PWR -> distributePower (behavior), R-HMI -> hmiPanel (structure), R-SEQ -> sequenceRefueling (behavior)
//   allocate : renderDisplay (behavior) -> hmiPanel (structure)
//   verify   : verification def objective verifies the 3 requirements

// --- Need (stakeholder) ---
requirement <'N-OPS'> 'Reliable Refueling Operations';

// --- Requirements (usages, not defs) ---
requirement <'R-PWR'> 'Power Distribution';
requirement <'R-HMI'> 'Operator Display';
requirement <'R-SEQ'> 'Refueling Sequencing';

// --- Structural anchors (untyped part usages) ---
part powerModule;
part hmiPanel;
part flightController;

// --- Behavioral anchors (untyped action usages) ---
action distributePower;
action renderDisplay;
action sequenceRefueling;

// --- Verification: objective verifies the 3 requirements ---
verification def Verify_Demo {
  objective {
    verify 'Power Distribution';
    verify 'Operator Display';
    verify 'Refueling Sequencing';
  }
}

// --- Cross-pillar traceability ---
// derive: requirements decompose the Need
dependency from 'Power Distribution' to 'Reliable Refueling Operations';
dependency from 'Operator Display' to 'Reliable Refueling Operations';
dependency from 'Refueling Sequencing' to 'Reliable Refueling Operations';

// satisfy: requirement -> realizing usage (function or component)
satisfy 'Power Distribution' by distributePower;
satisfy 'Operator Display' by hmiPanel;
satisfy 'Refueling Sequencing' by sequenceRefueling;

// allocate: function -> component
allocate renderDisplay to hmiPanel;
