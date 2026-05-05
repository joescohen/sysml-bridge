export interface SysONElement {
  '@id': string;
  '@type': string;
  declaredName?: string | null;
  declaredShortName?: string | null;
  name?: string | null;
  ownedElement?: Array<{ '@id': string }>;
  owner?: { '@id': string } | null;
  connectorEnd?: Array<{ connectedFeature?: { '@id': string } }>;
  source?: Array<{ '@id': string }> | { '@id': string } | null;
  target?: Array<{ '@id': string }> | { '@id': string } | null;
}

export interface Project {
  '@id': string;
  name: string;
}

export interface Representation {
  id: string;
  label: string;
  kind: string;
  targetObjectId?: string;
}
