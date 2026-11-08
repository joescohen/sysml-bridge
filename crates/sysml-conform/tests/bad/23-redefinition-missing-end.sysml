package P {
    part def A { part x; }
    part def B :> A {
        part y :>> ;
    }
}
