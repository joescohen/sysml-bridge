export interface SmapsResponse {
  "@id"?: string;
  "@type"?: string;
  [key: string]: unknown;
}

export interface SmapsElementResponse {
  "@id"?: string;
  "@type"?: string;
  name?: string;
  source?: Array<{ "@id": string }>;
  target?: Array<{ "@id": string }>;
  [key: string]: unknown;
}

export interface SmapsCommitResponse {
  "@id": string;
  "@type": "Commit";
  change: SmapsElementResponse[];
}

export interface SmapsProjectResponse {
  "@id": string;
  "@type": "Project";
  name: string;
}
