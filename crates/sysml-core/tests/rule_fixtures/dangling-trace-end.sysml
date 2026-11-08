package F {
    package SysMLv1 {
        metadata def Trace;
    }
    requirement r;
    part def Q;
    part p : Q;
    satisfy r by p;
    verification def V { objective { verify r; } }
    #SysMLv1::Trace dependency from p to Nope;
}
