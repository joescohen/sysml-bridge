package K {
    package SysMLv1 {
        metadata def Trace;
        metadata def DeriveReqt;
        metadata def ItemFlow;
    }
    item def Heat {
        attribute degrees;
    }
    item def VentCmd;
    item def Halt;
    port def Wire {
        inout item payload;
    }
    requirement <'N-1'> need {
        doc /* The potter needs each load fired evenly. */
    }
    requirement <'K-1'> fire {
        doc /* The kiln shall reach 1,200 °C (T) within 90 min and hold it for 15 min (O). */
        attribute peak = 1200;
    }
    requirement <'K-2'> cool {
        doc /* The kiln shall open the vent when the load is fired. */
    }
    requirement <'K-3'> shell {
        doc /* The cabinet shall weigh at most 40 kg. */
    }
    action def Measure {
        out heat : Heat;
    }
    action def Judge {
        in heat : Heat;
        out cmd : VentCmd;
    }
    action def OpenVent {
        in cmd : VentCmd;
    }
    action def Firing {
        action measure : Measure {
            out heat :>> Measure::heat;
        }
        action judge : Judge {
            in heat :>> Judge::heat;
            out cmd :>> Judge::cmd;
        }
        action vent : OpenVent {
            in cmd :>> OpenVent::cmd;
        }
        first start then measure;
        first measure then judge;
        first judge then vent;
        flow from measure.heat to judge.heat;
        flow from judge.cmd to vent.cmd;
    }
    action def Shutdown {
        action stop : OpenVent {
            in cmd :>> OpenVent::cmd;
        }
        first start then stop;
    }
    part def Probe {
        attribute mass = 2 [kg];
    }
    part def Brain {
        attribute mass = 3 [kg];
    }
    part def Flap;
    part def Guard;
    part def Chamber {
        part probe : Probe;
        part flap : Flap;
        port bus : Wire;
    }
    part def Cabinet {
        part brain : Brain;
        port bus : ~Wire;
    }
    part def Door {
        part guard : Guard;
    }
    part def Plant {
        part chamber : Chamber;
        part cabinet : Cabinet;
        part door : Door;
        connection trunk connect chamber.bus to cabinet.bus;
    }
    state def Modes {
        state idle;
        state firing {
            do action run {
                action measure : Measure {
                    out heat :>> Measure::heat;
                }
            }
        }
        transition first idle accept Heat then firing;
    }
    verification verifyFire {
        objective {
            verify fire;
        }
    }
    allocate Measure to Probe;
    allocate Judge to Brain;
    allocate OpenVent to Flap;
    allocate Plant::trunk to Judge;
    satisfy fire by Measure;
    satisfy cool by OpenVent;
    #SysMLv1::DeriveReqt dependency from fire to need;
    #SysMLv1::DeriveReqt dependency from cool to fire;
    #SysMLv1::ItemFlow dependency 'heat frame' from Chamber to Cabinet {
        comment conveyed about Heat /* the reading */
        comment realizedBy about Plant::trunk /* the trunk */
    }
    #SysMLv1::ItemFlow dependency 'vent frame' from Cabinet to Chamber {
        comment conveyed about VentCmd /* the command */
        comment realizedBy about Plant::trunk /* the trunk */
    }
}
