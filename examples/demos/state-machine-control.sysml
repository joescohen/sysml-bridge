// ANGARS C&C operating-mode state machine: entry/do/exit behaviors +
// transitions with trigger / guard / effect.
state def 'C&C Mode' {
    state Idle;
    state Authenticating {
        do verifyAircraft;
    }
    state Scheduling {
        do computeSchedule;
    }
    state Refueling {
        entry openValve;
        do executeRefuel;
        exit closeValve;
    }
    state Reporting;
    state Emergency {
        entry triggerAlarm;
    }
    state Shutdown;

    transition first Idle accept requestReceived then Authenticating;
    transition first Authenticating if authenticated then Scheduling;
    transition first Scheduling then Refueling;
    transition first Refueling if refuelComplete then Reporting;
    transition first Reporting then Idle;
    transition first Refueling accept faultDetected then Emergency;
    transition first Emergency if reset then Idle;
    transition first Reporting if missionComplete then Shutdown;
}
