export interface RequestOptions extends Omit<RequestInit, 'body' | 'headers'> {
  params?: Record<string, string | number | boolean | undefined | null>;
  body?: unknown;
  headers?: Record<string, string>;
}

export interface ApiError {
  status: number;
  message: string;
  data?: unknown;
}

export class Http {
  private baseURL: string;
  private defaultHeaders: Record<string, string>;

  constructor(baseURL: string, defaultHeaders: Record<string, string> = {}) {
    this.baseURL = baseURL.replace(/\/+$/, '');
    this.defaultHeaders = {
      'Content-Type': 'application/json',
      ...defaultHeaders,
    };
  }

  setAuthToken(token: string | null): void {
    if (token) {
      this.defaultHeaders['Authorization'] = `Bearer ${token}`;
    } else {
      delete this.defaultHeaders['Authorization'];
    }
  }

  async request<T = unknown>(endpoint: string, options: RequestOptions = {}): Promise<T> {
    const { params, body, headers, ...customOptions } = options;

    const cleanEndpoint = endpoint.startsWith('/') ? endpoint : `/${endpoint}`;
    const url = new URL(`${this.baseURL}${cleanEndpoint}`);

    if (params) {
      Object.entries(params).forEach(([key, value]) => {
        if (value !== undefined && value !== null) {
          url.searchParams.append(key, String(value));
        }
      });
    }

    const config: RequestInit = {
      ...customOptions,
      headers: {
        ...this.defaultHeaders,
        ...headers,
      },
    };

    if (body !== undefined) {
      if (body instanceof FormData) {
        delete (config.headers as Record<string, string>)['Content-Type'];
        config.body = body;
      } else {
        config.body = JSON.stringify(body);
      }
    }

    try {
      const response = await fetch(url.toString(), config);

      if (response.status === 204) {
        return null as T;
      }

      const data = await response.json().catch(() => null);

      if (!response.ok) {
        const error: ApiError = {
          status: response.status,
          message: data?.message || `HTTP Error! Status: ${response.status}`,
          data,
        };
        throw error;
      }

      return data as T;
    } catch (error) {
      console.error('[API Error]:', error);
      throw error;
    }
  }

  get<T = unknown>(endpoint: string, params?: RequestOptions['params'], headers?: Record<string, string>): Promise<T> {
    return this.request<T>(endpoint, { method: 'GET', params, headers });
  }

  post<T = unknown>(endpoint: string, body?: unknown, headers?: Record<string, string>): Promise<T> {
    return this.request<T>(endpoint, { method: 'POST', body, headers });
  }

  patch<T = unknown>(endpoint: string, body?: unknown, headers?: Record<string, string>): Promise<T> {
    return this.request<T>(endpoint, { method: 'PATCH', body, headers });
  }

  delete<T = unknown>(endpoint: string, headers?: Record<string, string>): Promise<T> {
    return this.request<T>(endpoint, { method: 'DELETE', headers });
  }
}
