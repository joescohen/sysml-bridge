// PILLAR: Structural -- Command & Control INTERNAL BLOCK DIAGRAM (complex)
// A subsystem block with 6 internal parts, multiple ports each, and a web of connections.
// Cross-pillar: satisfy req->component, allocate function->component.
port def PowerPort;
port def DataPort;
port def SignalPort;
part def 'Battery';
part def 'Power Controller';
part def 'Flight Controller';
part def 'HMI Panel';
part def 'Sensor Array';
part def 'Comms Module';
part def 'C&C Subsystem' {
  part battery : 'Battery' {
    port pwrOut : PowerPort;
  }
  part powerCtrl : 'Power Controller' {
    port pwrIn : PowerPort;
    port pwrA : PowerPort;
    port pwrB : PowerPort;
  }
  part flightCtrl : 'Flight Controller' {
    port pwrIn : PowerPort;
    port dataIn : DataPort;
    port dataOut : DataPort;
  }
  part hmiPanel : 'HMI Panel' {
    port pwrIn : PowerPort;
    port dataIn : DataPort;
  }
  part sensors : 'Sensor Array' {
    port sigOut : SignalPort;
  }
  part comms : 'Comms Module' {
    port dataIn : DataPort;
    port sigIn : SignalPort;
  }
  connect battery.pwrOut to powerCtrl.pwrIn;
  connect powerCtrl.pwrA to flightCtrl.pwrIn;
  connect powerCtrl.pwrB to hmiPanel.pwrIn;
  connect flightCtrl.dataOut to hmiPanel.dataIn;
  connect flightCtrl.dataOut to comms.dataIn;
  connect sensors.sigOut to comms.sigIn;
}
