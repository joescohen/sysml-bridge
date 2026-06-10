// Cross-pillar traceability slice: need <- requirement -> function -> component,
// plus verification. Exercises all four trace kinds (derive/satisfy/allocate/verify).
package 'C&C Trace' {
    requirement <'N1'> 'Operational Need';
    requirement <'R1'> 'Aircraft ID Verification';
    requirement <'R2'> 'Fuel Capacity Check';
    action 'Receive Request';
    action 'Validate Fuel';
    part 'Flight Controller';
    verification def 'ID Test' {
        objective {
            verify 'Aircraft ID Verification';
        }
    }

    dependency from 'Aircraft ID Verification' to 'Operational Need';
    dependency from 'Fuel Capacity Check' to 'Operational Need';
    satisfy 'Aircraft ID Verification' by 'Receive Request';
    satisfy 'Fuel Capacity Check' by 'Validate Fuel';
    allocate 'Receive Request' to 'Flight Controller';
    allocate 'Validate Fuel' to 'Flight Controller';
}
