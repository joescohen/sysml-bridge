export interface SmapsProject {
  "@id": string;
  "@type": "Project";
  name: string;
  description?: string;
  created?: string;
  defaultBranch?: { "@id": string };
}

export interface SmapsDataVersion {
  "@type": "DataVersion";
  payload: Record<string, unknown> | null;
  identity?: { "@id": string };
}

export interface SmapsCommitRequest {
  "@type": "Commit";
  change: SmapsDataVersion[];
  previousCommit?: { "@id": string };
}

export interface SmapsCommitResponse {
  "@id": string;
  "@type": "Commit";
  created: string;
  owningProject: { "@id": string };
  previousCommit?: { "@id": string };
  change: SmapsDataVersion[];
}

export interface SmapsElementResponse {
  "@id": string;
  "@type": string;
  name?: string | null;
  declaredName?: string | null;
  declaredShortName?: string | null;
  elementId?: string;
  qualifiedName?: string;
  owner?: { "@id": string };
  ownedElement?: Array<{ "@id": string }>;
  ownedRelationship?: Array<{ "@id": string }>;
  source?: Array<{ "@id": string }>;
  target?: Array<{ "@id": string }>;
  relatedElement?: Array<{ "@id": string }>;
  [key: string]: unknown;
}

export interface SmapsPrimitiveConstraint {
  "@type": "PrimitiveConstraint";
  inverse: boolean;
  operator: "=" | "<" | "<=" | ">" | ">=" | "in" | "instanceOf";
  property: string;
  value: string | number | boolean;
}

export interface SmapsCompositeConstraint {
  "@type": "CompositeConstraint";
  operator: "and" | "or";
  constraint: Array<SmapsPrimitiveConstraint | SmapsCompositeConstraint>;
}

export interface SmapsQuery {
  "@type": "Query";
  select?: string[];
  where?: SmapsPrimitiveConstraint | SmapsCompositeConstraint;
}

export interface SmapsBranch {
  "@id": string;
  "@type": "Branch";
  name: string;
  head: { "@id": string };
  owningProject: { "@id": string };
  created: string;
}
