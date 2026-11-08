package F {
    item def I;
    port def P { out item data : I; }
    part def Inner { port p : P; }
    part def Top {
        port p : P;
        part inner : Inner;
        connect p to inner.p;
    }
}
