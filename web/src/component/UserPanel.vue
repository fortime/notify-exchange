<template>
  <BNavItemDropdown
    :text="userEmail"
    right
  >
    <BDropdownItem @click="logout">
      Logout
    </BDropdownItem>
  </BNavItemDropdown>
</template>

<script setup>
import { computed } from 'vue'
import { useRouter } from 'vue-router'
import { state } from '../service/state'
import apiClient from '../service/api'
import { BDropdownItem, BNavItemDropdown } from 'bootstrap-vue-next'
import { useToast } from 'vue-toast-notification'

const router = useRouter()
const userEmail = computed(() => state.user?.email)
const $toast = useToast()

async function logout() {
  try {
    await apiClient.delete('/v1/user/session')
  } catch (error) {
    console.error('Logout failed:', error)
    $toast.error('Logout failed: ' + (error.response?.data?.message || error.message))
    // We clear state and redirect anyway
  } finally {
    const logoutRoute = state.logoutRouteName;
    // Clear local state
    state.isLoggedIn = false
    state.user = null
    localStorage.removeItem('csrfToken');
    state.logoutRouteName = null;
    // Redirect to login
    if (logoutRoute) {
      router.push({ name: logoutRoute });
    } else {
      router.push('/login');
    }
  }
}
</script>
