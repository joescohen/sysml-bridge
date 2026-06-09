// PILLAR: Structural (BDD/IBD -- parts, ports, connections) -- Aerial Refueling Control
// Cross-pillar links demonstrated:
//   satisfy  : R-PWR -> powerModule, R-HMI -> hmiPanel, R-SEQ -> flightController (req -> component part usage)
//   allocate : distributePower -> powerModule, renderDisplay -> hmiPanel, sequenceRefueling -> flightController
//   IBD      : connect powerModule.pwrOut to flightController.pwrIn, etc.

// --- Port definitions (interface types) ---
port def PowerPort;
port def DataPort;

// --- Part definitions (BDD blocks) ---
part def 'Power Module';
part def 'HMI Panel';
part def 'Flight Controller';

// --- Typed part usages with ports (IBD endpoints, package-level) ---
part powerModule : 'Power Module' {
  port pwrOut : PowerPort;
}
part hmiPanel : 'HMI Panel' {
  port dataIn : DataPort;
}
part flightController : 'Flight Controller' {
  port pwrIn : PowerPort;
  port dataOut : DataPort;
}

// --- Connections (IBD) ---
connect powerModule.pwrOut to flightController.pwrIn;
connect flightController.dataOut to hmiPanel.dataIn;

// --- Requirement anchors (usages) ---
requirement <'R-PWR'> 'Power Distribution';
requirement <'R-HMI'> 'Operator Display';
requirement <'R-SEQ'> 'Refueling Sequencing';

// --- Behavioral anchors (untyped action usages) ---
action distributePower;
action renderDisplay;
action sequenceRefueling;

// --- Cross-pillar traceability ---
// satisfy: requirement -> component part usage
satisfy 'Power Distribution' by powerModule;
satisfy 'Operator Display' by hmiPanel;
satisfy 'Refueling Sequencing' by flightController;

// allocate: function action usage -> component part usage
allocate distributePower to powerModule;
allocate renderDisplay to hmiPanel;
allocate sequenceRefueling to flightController;
