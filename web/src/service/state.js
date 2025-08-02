import { reactive } from 'vue';

export const state = reactive({
  isInitialized: null, // null: unknown, true: initialized, false: not initialized
  isLoggedIn: false,
  user: null,
});
