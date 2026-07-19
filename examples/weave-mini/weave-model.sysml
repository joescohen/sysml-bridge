package 'Cargo Handling Requirements' {  // @id: wm::requirements-container
  requirement 'REQ-001';  // @source: REQ-001 @id: entity-fe1b1bb5187b79a2
  requirement 'REQ-002';  // @source: REQ-002 @id: entity-fea4523b2b20ff49
  requirement 'REQ-003';  // @source: REQ-003 @id: entity-fd6a1d229ff0adac
  requirement 'REQ-004';  // @source: REQ-004 @id: entity-b4f59f9c508b5283
  requirement 'REQ-005';  // @source: REQ-005 @id: entity-b7b2cf86c099b70d
  requirement 'REQ-006';  // @source: REQ-006 @id: entity-ce832d3dbc2dbf4c
  requirement 'REQ-007';  // @source: REQ-007 @id: entity-4c3863f667113626
  requirement 'REQ-008';  // @source: REQ-008 @id: entity-12187188419cbfcf
  requirement 'REQ-009';  // @source: REQ-009 @id: entity-be1bce839c77abbe
  requirement 'REQ-010';  // @source: REQ-010 @id: entity-bd56d7687e819795
  requirement 'N-001';  // @source: N-001 @id: entity-38c99905440cf025
}
part 'Cargo Handling Subsystem' {  // @id: wm::components-container
  part 'Cargo Handling Controller' {  // @id: entity-d4196a07ebb75d55
    part 'Position Sensor Array';  // @source: wm-accepted-r2 @id: entity-4761bdf7260ef65f
  }
  part CHC;  // @source: unmerged-acronym-alias-pending-review @id: entity-78fee0e1101d46c4
  part 'Conveyor Drive Motor';  // @id: entity-ed308afa48547c14
  part 'Load Cell Assembly';  // @id: entity-312ac845a5db0c7a
  part Interlock;  // @id: entity-23a7d494a0f7e515
  part 'Boom Actuator';  // @id: entity-88dd6ef9c47bc704
  part 'Fault Logger';  // @id: entity-c0cba29ae877092f
  part 'Diagnostic Interface';  // @id: entity-ae1c48a2b87c1ff5
}
action 'Cargo Handling Operations' {  // @id: wm::functions-container
  action 'Detect Cargo Presence';  // @id: entity-40ac5107b4d6ec8f
  action 'Compute Load Distribution';  // @id: entity-9ade89edcd2d432f
  action 'Monitor Conveyor Speed';  // @id: entity-08ec4f4cc7d32731
  action 'Validate Load Capacity';  // @id: entity-6f1fdc71889bc2d2
  action 'Log Fault Event';  // @id: entity-c32b3c3cf4de76e7
  action 'Transmit Fault Summary';  // @id: entity-b9810b3942ea9056
}
state def 'Cargo Handling Modes' {  // @id: wm::modes-container
  state 'Interlock Mode';  // @id: entity-d3825c77d70ec9a2
  state Standby;  // @id: entity-a669add53e3253a6
}
verification def Verify_REQ_001 {  // @id: wm::verify-req001
  objective {
    verify 'REQ-001';
  }
}

package 'C&C Trace' {
  // traceability
  satisfy 'REQ-001' by 'Detect Cargo Presence';
  dependency from 'REQ-001' to 'N-001';
  allocate 'Detect Cargo Presence' to 'Position Sensor Array';
  allocate 'Detect Cargo Presence' to 'Cargo Handling Controller';
}
