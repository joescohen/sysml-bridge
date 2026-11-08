package P {
    part def A { attribute m : ScalarValues::Real; }
    part a : A;
    part b : A;
    // Cameo's Satisfy names a requirement as its supplier (customization typesForTarget = AbstractRequirement).
    satisfy b by a;
}
