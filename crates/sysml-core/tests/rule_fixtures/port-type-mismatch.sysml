package F {
    item def I;
    port def Q1 { out item i : I; }
    port def Q2 { out item j : I; }
    part def A { port x : Q1; }
    part def B { port y : ~Q2; }
    part def Top {
        part a : A;
        part b : B;
        connect a.x to b.y;
    }
}
