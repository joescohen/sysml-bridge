// coverage_owner: grouping
package F {
    part def Part;
    requirement r;
    satisfy r by Part;
    #SysMLv1::Verify dependency from Part to r;
    #SysMLv1::Refine dependency from Part to r;
}
