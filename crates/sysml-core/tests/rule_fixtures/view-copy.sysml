package F {
    port def PowerPort;
    port def DataPort;
    port def FluidPort;
    part def Pump { port power : PowerPort; port data : DataPort; port inlet : FluidPort; port outlet : FluidPort; }
    part def PumpFunctional { port inlet : FluidPort; port outlet : FluidPort; }
}
