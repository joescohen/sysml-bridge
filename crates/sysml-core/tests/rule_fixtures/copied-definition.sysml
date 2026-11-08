package F {
    item def Frame;
    part def Camera { port lens : Frame; attribute mass; attribute focus; }
    part def Imager { port lens : Frame; attribute mass; attribute focus; }
}
