package F {
    part def Base { port p; }
    part def Inner :> Base;
    part def Middle { part inner : Inner; }
    part def Top {
        part middle : Middle;
        port p;
        connect middle.inner.p to p;
    }
}
