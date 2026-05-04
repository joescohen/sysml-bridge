export interface SmapsElement {
  '@id': string;
  '@type': string;
  declaredName?: string;
  declaredShortName?: string;
  name?: string;
}

export interface ConnectorEnd {
  '@type': 'ConnectorEnd';
  connectedFeature: { '@id': string };
}

export interface LocalElement extends SmapsElement {
  _local: true;
  owner?: { '@id': string };
  type?: Array<{ '@id': string }>;
  connectorEnd?: ConnectorEnd[];
}

export interface Project {
  '@id': string;
  name: string;
}

export interface StoredDiagram {
  type: string;
  title: string;
  mermaid: string;
  updatedAt: string;
}
