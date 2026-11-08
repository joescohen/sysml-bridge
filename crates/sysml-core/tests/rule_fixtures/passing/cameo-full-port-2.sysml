package P {
    part def Body;
    part def Holder {
        port p : Body;
        ref part piece : Body;
        bind p = piece;
    }
}
