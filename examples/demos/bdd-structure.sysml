// ANGARS C&C structural decomposition (BDD): part defs with attribute
// compartments, composition (owner -> contained type), multiplicity, and
// specialization (:>).
package 'C&C Architecture' {
    part def 'C&C Subsystem' {
        attribute powerBudget : Real;
        part power : 'Power Module';
        part flightCtrl : 'Flight Controller'[2];
        part hmi : 'HMI Panel';
    }
    part def 'Power Module' {
        attribute voltage : Real;
        attribute capacity : Real;
    }
    part def 'Flight Controller' {
        attribute clockRate : Real;
    }
    part def 'HMI Panel' {
        attribute refreshRate : Real;
    }
    part def 'Redundant Power Module' :> 'Power Module' {
        attribute backupCells : Integer;
    }
}
