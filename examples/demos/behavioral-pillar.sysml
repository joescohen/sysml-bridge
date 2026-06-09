// PILLAR: Behavioral (Activity actions+flows + State machine transitions) -- Aerial Refueling Control
// Cross-pillar links demonstrated:
//   satisfy  : R-SEQ -> sequenceRefueling, R-PWR -> distributePower, R-HMI -> renderDisplay (req -> function action usage)
//   allocate : distributePower -> powerModule, renderDisplay -> hmiPanel, sequenceRefueling -> flightController
//   activity : action def 'Manage Refueling' with first/then successions + flow
//   state    : state def 'Refueling Modes' with transitions

// --- Activity: action def with successions and flow ---
action def 'Manage Refueling' {
  action receive;
  action validate;
  action sequence;
  first receive then validate;
  first validate then sequence;
  flow from receive to validate;
  flow from validate to sequence;
}

// --- State machine: state def with transitions ---
state def 'Refueling Modes' {
  state idle;
  state active;
  state complete;
  transition first idle then active;
  transition first active then complete;
}

// --- Leaf function action usages (package-level, trace anchors) ---
action distributePower;
action renderDisplay;
action sequenceRefueling;

// --- Requirement anchors (usages) ---
requirement <'R-PWR'> 'Power Distribution';
requirement <'R-HMI'> 'Operator Display';
requirement <'R-SEQ'> 'Refueling Sequencing';

// --- Component anchors (untyped part usages) ---
part powerModule;
part hmiPanel;
part flightController;

// --- Cross-pillar traceability ---
// satisfy: requirement -> function action usage
satisfy 'Power Distribution' by distributePower;
satisfy 'Operator Display' by renderDisplay;
satisfy 'Refueling Sequencing' by sequenceRefueling;

// allocate: function action usage -> component part usage
allocate distributePower to powerModule;
allocate renderDisplay to hmiPanel;
allocate sequenceRefueling to flightController;
