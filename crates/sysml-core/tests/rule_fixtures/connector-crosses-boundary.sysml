package F {
    port def Data;
    part def Cpu { port in1 : ~Data; }
    part def Board { part cpu : Cpu; }
    part def Sensor { port out1 : Data; }
    part def Drone {
        part sensor : Sensor;
        part board : Board;
        connect sensor.out1 to board.cpu.in1;
    }
}
