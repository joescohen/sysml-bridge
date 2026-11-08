package P {
    part def Body;
    part def Base { port old : Body; }
    part def Derived :> Base { port p : Body :>> old; }
}
