// ANGARS-flavored Action Flow exercising control nodes + guards:
//   decide (decision) with two guarded branches, fork/join (parallel), merge.
action def 'Refueling Request Handling' {
    action 'Receive Request';
    action 'Validate Fuel Capacity';
    decide fuelCheck;
    action 'Reject Request';
    action 'Prioritize Requests';
    fork dispatch;
    action 'Generate Schedule';
    action 'Display Mission Data';
    join sync;
    merge done;
    action 'Transmit Status & Reports';

    first 'Receive Request' then 'Validate Fuel Capacity';
    first 'Validate Fuel Capacity' then fuelCheck;
    first fuelCheck if fuelSufficient then 'Prioritize Requests';
    first fuelCheck if fuelLow then 'Reject Request';
    first 'Prioritize Requests' then dispatch;
    first dispatch then 'Generate Schedule';
    first dispatch then 'Display Mission Data';
    first 'Generate Schedule' then sync;
    first 'Display Mission Data' then sync;
    first sync then 'Transmit Status & Reports';
    first 'Transmit Status & Reports' then done;
    first 'Reject Request' then done;
}
