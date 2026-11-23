import { Http } from "./http";

export default class LazypockClient {
  http: Http

  constructor(baseURL: string, defaultHeaders: Record<string, string> = {}) {
    this.http = new Http(baseURL, defaultHeaders)
  }

}
