import type { SysmlElement, SysmlRelationship, ProjectState } from "./types/sysml-elements.js";
import type { SmapsResponse, SmapsElementResponse } from "./types/smaps.js";

export class SmapsClient {
  private endpoint: string;
  private projectId: string;
  private commitId: string | null = null;

  constructor(endpoint: string, projectId: string) {
    this.endpoint = endpoint.replace(/\/$/, "");
    this.projectId = projectId;
  }

  async checkConnection(): Promise<boolean> {
    try {
      const res = await fetch(`${this.endpoint}/projects`);
      return res.ok;
    } catch {
      return false;
    }
  }

  async ensureProject(): Promise<string> {
    const res = await fetch(`${this.endpoint}/projects/${this.projectId}`);
    if (res.ok) return this.projectId;

    const createRes = await fetch(`${this.endpoint}/projects`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ "@id": this.projectId, name: this.projectId }),
    });
    if (!createRes.ok) {
      throw new Error(`Failed to create project: ${createRes.statusText}`);
    }
    return this.projectId;
  }

  async createElement(
    type: string,
    name: string,
    attributes: Record<string, unknown> = {}
  ): Promise<SysmlElement> {
    const connected = await this.checkConnection();
    if (!connected) {
      throw new Error(
        `SMAPS server not reachable at ${this.endpoint} — run 'docker compose up' in the sysml-bridge/docker directory`
      );
    }

    await this.ensureProject();

    const element = {
      "@type": type,
      name,
      ...attributes,
    };

    const res = await fetch(
      `${this.endpoint}/projects/${this.projectId}/commits`,
      {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ change: [{ payload: element }] }),
      }
    );

    if (!res.ok) {
      throw new Error(`Failed to create element: ${res.statusText}`);
    }

    const data = (await res.json()) as SmapsElementResponse;
    return this.toSysmlElement(data);
  }

  async getElement(elementId: string): Promise<SysmlElement> {
    const res = await fetch(
      `${this.endpoint}/projects/${this.projectId}/elements/${elementId}`
    );
    if (!res.ok) {
      throw new Error(`Element not found: ${elementId}`);
    }
    const data = (await res.json()) as SmapsElementResponse;
    return this.toSysmlElement(data);
  }

  async queryElements(
    type?: string,
    namePattern?: string
  ): Promise<SysmlElement[]> {
    const params = new URLSearchParams();
    if (type) params.set("type", type);
    if (namePattern) params.set("name", namePattern);

    const res = await fetch(
      `${this.endpoint}/projects/${this.projectId}/elements?${params}`
    );
    if (!res.ok) {
      throw new Error(`Query failed: ${res.statusText}`);
    }
    const data = (await res.json()) as SmapsElementResponse[];
    return data.map((d) => this.toSysmlElement(d));
  }

  async updateElement(
    elementId: string,
    updates: Record<string, unknown>
  ): Promise<SysmlElement> {
    const res = await fetch(
      `${this.endpoint}/projects/${this.projectId}/elements/${elementId}`,
      {
        method: "PATCH",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify(updates),
      }
    );
    if (!res.ok) {
      throw new Error(`Failed to update element: ${res.statusText}`);
    }
    const data = (await res.json()) as SmapsElementResponse;
    return this.toSysmlElement(data);
  }

  async deleteElement(elementId: string): Promise<void> {
    const res = await fetch(
      `${this.endpoint}/projects/${this.projectId}/elements/${elementId}`,
      { method: "DELETE" }
    );
    if (!res.ok) {
      throw new Error(`Failed to delete element: ${res.statusText}`);
    }
  }

  async createRelationship(
    type: string,
    sourceId: string,
    targetId: string,
    attributes: Record<string, unknown> = {}
  ): Promise<SysmlRelationship> {
    const element = {
      "@type": type,
      source: [{ "@id": sourceId }],
      target: [{ "@id": targetId }],
      ...attributes,
    };

    const res = await fetch(
      `${this.endpoint}/projects/${this.projectId}/commits`,
      {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ change: [{ payload: element }] }),
      }
    );

    if (!res.ok) {
      throw new Error(`Failed to create relationship: ${res.statusText}`);
    }

    const data = (await res.json()) as SmapsElementResponse;
    return {
      id: data["@id"] ?? "",
      type,
      sourceId,
      targetId,
    };
  }

  async queryRelationships(
    elementId?: string,
    type?: string
  ): Promise<SysmlRelationship[]> {
    const params = new URLSearchParams();
    if (type) params.set("type", type);
    if (elementId) params.set("relatedElement", elementId);

    const res = await fetch(
      `${this.endpoint}/projects/${this.projectId}/elements?${params}`
    );
    if (!res.ok) {
      throw new Error(`Relationship query failed: ${res.statusText}`);
    }
    const data = (await res.json()) as SmapsElementResponse[];
    return data.map((d) => ({
      id: d["@id"] ?? "",
      type: d["@type"] ?? "",
      sourceId: d.source?.[0]?.["@id"] ?? "",
      targetId: d.target?.[0]?.["@id"] ?? "",
    }));
  }

  async getProjectState(): Promise<ProjectState> {
    const elements = await this.queryElements();

    const counts: Record<string, number> = {};
    for (const el of elements) {
      counts[el.type] = (counts[el.type] ?? 0) + 1;
    }

    return {
      projectId: this.projectId,
      totalElements: elements.length,
      elementCountsByType: counts,
    };
  }

  private toSysmlElement(data: SmapsElementResponse): SysmlElement {
    return {
      id: data["@id"] ?? "",
      type: data["@type"] ?? "",
      name: data.name ?? "",
      attributes: data,
    };
  }
}
