package DroneSystem {

    // Part Definitions (BDD)
    part def Drone;
    part def Airframe;
    part def PropulsionSystem;
    part def Avionics;
    part def PayloadBay;
    part def BrushlessMotor;
    part def LiPoBattery;
    part def FlightController;
    part def GPSModule;

    // Requirements
    requirement def <'SYS-001'> MaxTakeoffWeight {
        doc /* The drone shall not exceed 25 kg maximum takeoff weight */
    }

    requirement def <'SYS-002'> FlightEndurance {
        doc /* The drone shall maintain flight for at least 30 minutes */
    }

    requirement def <'SYS-003'> OperationalRange {
        doc /* The drone shall operate within a 5 km radius */
    }

    // System composition
    part drone : Drone {
        part airframe : Airframe;
        part propulsion : PropulsionSystem {
            part motor : BrushlessMotor;
            part battery : LiPoBattery;
        }
        part avionics : Avionics {
            part flightController : FlightController;
            part gps : GPSModule;
        }
        part payloadBay : PayloadBay;
    }
}
