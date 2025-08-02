import axios from 'axios';
import { state } from './state';
import router from '../router';

const apiClient = axios.create({
  baseURL: process.env.VUE_APP_API_BASE_PATH,
  headers: {
    'Content-Type': 'application/json'
  }
});

// Add a request interceptor to include the CSRF token
apiClient.interceptors.request.use(
  (config) => {
    const csrfToken = localStorage.getItem('csrfToken');
    if (csrfToken) {
      config.headers['x-ne-csrf-token'] = csrfToken;
    }
    return config;
  },
  (error) => {
    return Promise.reject(error);
  }
);

// Add a response interceptor to handle 401 errors
apiClient.interceptors.response.use(
  (response) => {
    return response;
  },
  (error) => {
    if (error.response && error.response.status === 401) {
      state.isLoggedIn = false;
      state.user = null;
      localStorage.removeItem('csrfToken');
    }
    return Promise.reject(error);
  }
);

export default apiClient;

