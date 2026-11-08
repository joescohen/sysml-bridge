package P {
    part def A;
    part def B;
    part def Sys {
        part a : A;
        part b : B;
        // The two parts are joined, and the model does not say through which ports.
        connect a to b;
    }
}
