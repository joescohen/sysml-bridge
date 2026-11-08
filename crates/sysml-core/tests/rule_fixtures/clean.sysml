// The clean model: it breaks no rule of any gate.
package Clean {
    private import SI::*;
    attribute supply = 12 [V];
    package SysMLv1 {
        metadata def Actor;
        metadata def logical { attribute atomic; }
        metadata def ValueType { attribute unit; }
        metadata def functionalRequirement;
        metadata def performanceRequirement;
        metadata def Refine;
        metadata def Trace;
    }
    attribute def Metre {
        @SysMLv1::ValueType { unit = "m"; }
    }
    enum def Mode { cruise; idle; }
    item def Command;
    port def Link { out item sig : Command; }
    port def Wrap { port inner : Link; }
    #SysMLv1::Actor part def Pilot { doc /* The person who drives the vehicle. */ }
    part def Engine {
        @SysMLv1::logical { atomic = true; }
        action ignite { doc /* Start the engine. */ }
    }
    part def Motor {
        @SysMLv1::logical { atomic = true; }
        port feed : ~Link;
        action spin { doc /* Turn the motor. */ }
    }
    constraint def Limit {
        doc /* The value stays above zero. */
        in x : Metre;
        x > 0
    }
    part def Vehicle {
        doc /* The vehicle. */
        @SysMLv1::logical { atomic = false; }
        part engine : Engine;
        part motor : Motor;
        port drive : ~Link;
        attribute mass : Metre;
        connect drive to motor.feed;
        action run {
            doc /* Move the vehicle on a command. */
            in cmd : Command;
            out res : Command;
        }
        constraint limit : Limit { in x = 5; }
        state def Modes {
            state off;
            state on;
            transition first off accept when "power on" then on;
            transition first on accept after "5 s" then off;
            transition first on accept Command then off;
        }
        action def Cycle {
            action a1 {
                out o : Command;
                comment calledOperation about Clean::Vehicle::run /* Calls operation run */
            }
            action a2 { in i : Command; }
            action a3 { in i3 : Command; }
            perform action ignite ::> engine.ignite;
            perform action spin ::> motor.spin;
            first a3 then ignite;
            first ignite then spin;
            fork f;
            first start then a1;
            first a1 then f;
            first f then a2;
            first f then a3;
            flow from a1.o to a2.i;
        }
    }
    use case def Drive {
        doc /* Drive the vehicle. */
        actor pilot : Pilot;
    }
    requirement perfReq {
        doc /* The vehicle shall move at 100 km/h. */
        @SysMLv1::performanceRequirement;
    }
    requirement funcReq {
        doc /* The vehicle shall move. */
        @SysMLv1::functionalRequirement;
    }
    part vehicle1 : Vehicle;
    #SysMLv1::Refine dependency from perfReq to funcReq;
    #SysMLv1::Trace dependency from Drive to funcReq;
    satisfy perfReq by vehicle1.mass;
    satisfy funcReq by vehicle1.run;
    verification def Check {
        objective {
            verify perfReq;
            verify funcReq;
        }
    }
}
