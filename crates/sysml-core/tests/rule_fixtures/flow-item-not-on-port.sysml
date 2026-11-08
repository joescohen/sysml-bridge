package F {
    item def Status;
    item def Reading;
    port def BusPort { out item s : Status; }
    part def Fpga { port bus : BusPort; }
    part def Cpu { port bus : ~BusPort; }
    part def Board {
        part fpga : Fpga;
        part cpu : Cpu;
        connect fpga.bus to cpu.bus;
        flow of Reading from fpga.bus to cpu.bus;
    }
}
