import { createClient } from "./lazypock.types";

export const client = createClient({
  baseUrl: "http://localhost:4000/api",
});
