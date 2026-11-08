package F {
    item def I;
    port def Q { out item i : I; }
    port def R { out item j : I; }
    part def Sub { port x : Q; }
    part def SubB { port y : ~Q; }
    part def Top {
        part sub : Sub;
        part subb : SubB;
        connect Sub::x to subb.y;
    }
}
