// coverage_owner: grouping
package F {
    part def Part;
    requirement def Derived;
    requirement r;
    satisfy r by Part;
    #SysMLv1::Verify dependency from Part to r;
    #SysMLv1::Refine dependency from Part to r;
    #SysMLv1::DeriveReqt dependency from Derived to r;
}
