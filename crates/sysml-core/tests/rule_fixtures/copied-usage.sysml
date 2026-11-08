package F {
    action def Cycle { action step; }
    action def Step;
    action def Flow { action s : Step; }
    part def Pump { perform action s : Step; }
}
