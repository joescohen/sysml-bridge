package F {
    item def I;
    port def Q { out item i : I; }
    part def A { port x : Q; }
    part def Top {
        part a : A;
        part b : A;
        connect a.x to b.x;
    }
}
