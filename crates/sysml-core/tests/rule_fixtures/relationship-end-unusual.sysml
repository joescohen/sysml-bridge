package P {
    item def Sig;
    port def PD { out item s : Sig; }
    part def A { port p : PD; }
    part a : A;
    requirement def RD;
    requirement r1 : RD;
    // The language takes a port as a satisfier; the checker names it as unusual.
    satisfy r1 by a.p;
}
