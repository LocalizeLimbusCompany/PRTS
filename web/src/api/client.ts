import axios, { AxiosInstance, AxiosRequestConfig, AxiosResponse } from 'axios';

export interface ApiResponse<T = any> {
  success: boolean;
  data: T;
  error: {
    code: string;
    message: string;
  } | null;
  requestId: string;
}

export interface PaginatedData<T> {
  items: T[];
  page: number;
  pageSize: number;
  total: number;
}

const apiClient: AxiosInstance = axios.create({
  baseURL: '/api/v1',
  headers: {
    'Content-Type': 'application/json',
    'Accept': 'application/json',
  },
});

apiClient.interceptors.request.use((config) => {
  let token = localStorage.getItem('prts_token');
  
  // Attempt to extract from Zustand persist storage
  const authStorageStr = localStorage.getItem('auth-storage');
  if (authStorageStr) {
    try {
      const authStorage = JSON.parse(authStorageStr);
      if (authStorage?.state?.token) {
        token = authStorage.state.token;
      }
    } catch (e) {
      // ignore JSON parse errors
    }
  }

  if (token && config.headers) {
    config.headers.Authorization = `Bearer ${token}`;
  }
  return config;
});

apiClient.interceptors.response.use(
  (response: AxiosResponse<ApiResponse>) => {
    // If the server returns 200, but success is false, we might want to reject
    if (response.data && response.data.success === false) {
      return Promise.reject(response.data.error);
    }
    return response;
  },
  (error) => {
    return Promise.reject(error.response?.data?.error || { code: 'unknown', message: error.message });
  }
);

export const api = {
  get: <T>(url: string, config?: AxiosRequestConfig) => 
    apiClient.get<ApiResponse<T>>(url, config).then(res => res.data.data),
  post: <T>(url: string, data?: any, config?: AxiosRequestConfig) => 
    apiClient.post<ApiResponse<T>>(url, data, config).then(res => res.data.data),
  patch: <T>(url: string, data?: any, config?: AxiosRequestConfig) => 
    apiClient.patch<ApiResponse<T>>(url, data, config).then(res => res.data.data),
  delete: <T>(url: string, config?: AxiosRequestConfig) => 
    apiClient.delete<ApiResponse<T>>(url, config).then(res => res.data.data),
};