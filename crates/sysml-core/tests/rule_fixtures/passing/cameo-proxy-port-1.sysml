package P {
    port def Face;
    part def Base { port old : Face; }
    part def Derived :> Base { port p : Face :>> old; }
}
