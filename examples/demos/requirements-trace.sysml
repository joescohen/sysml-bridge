// ANGARS C&C requirements: requirement defs/usages with derive (dependency),
// satisfy (part -> req), and verify (verification case -> req).
package 'C&C Requirements' {
    requirement <'R1'> 'Power Management' {
        attribute statement = "The subsystem shall manage the power budget";
    }
    requirement <'R1.1'> 'Voltage Regulation' {
        attribute statement = "Bus voltage shall stay within 5%";
    }
    requirement <'R1.2'> 'Backup Power' {
        attribute statement = "Backup cells shall sustain 10 minutes";
    }
    part 'Power Module';
    part 'Battery';
    verification def 'Power Test' {
        objective {
            verify 'Power Management';
        }
    }

    dependency from 'Voltage Regulation' to 'Power Management';
    dependency from 'Backup Power' to 'Power Management';
    satisfy 'Voltage Regulation' by 'Power Module';
    satisfy 'Backup Power' by 'Battery';
}
