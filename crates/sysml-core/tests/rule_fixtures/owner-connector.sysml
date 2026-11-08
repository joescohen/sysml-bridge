package P { port def D; part def C { port a : D; port b : ~D; } part def S { part c : C; } connect S::c.a to S::c.b; }
