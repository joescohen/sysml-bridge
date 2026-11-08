package P {
    port def Inner;
    port def Face { port inner : Inner; }
    part def Holder { port p : Face; }
}
