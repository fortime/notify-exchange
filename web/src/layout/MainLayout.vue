<template>
  <BApp>
    <BNavbar
      id="navbar"
      v-b-color-mode="'dark'"
      variant="primary"
      toggleable="lg"
      container="lg"
    >
      <BNavbarBrand :to="{ name: 'Home' }">
        Notify Exchange
      </BNavbarBrand>
      <BNavbarToggle
        target="nav-collapse"
        v-if="isLoggedIn"
      />
      <BCollapse
        id="nav-collapse"
        is-nav
        v-if="isLoggedIn"
      >
        <BNavbarNav
          class="mb-2 mb-lg-0"
          v-if="state.user && state.user.is_admin"
        >
          <BNavItem :to="{ name: 'AdminPortal' }">
            Admin Portal
          </BNavItem>
        </BNavbarNav>
        <BNavbarNav class="ms-auto mb-2 mb-lg-0">
          <UserPanel />
        </BNavbarNav>
      </BCollapse>
    </BNavbar>
    <BContainer class="mt-4">
      <router-view />
    </BContainer>
  </BApp>
</template>

<script setup>
import { computed } from 'vue'
import { state } from '../service/state'
import UserPanel from '../component/UserPanel.vue'
import {
  BApp,
  BCollapse,
  BContainer,
  BNavbar,
  BNavbarBrand,
  BNavbarNav,
  BNavbarToggle,
  BNavItem,
  vBColorMode
} from 'bootstrap-vue-next'

const isLoggedIn = computed(() => state.isLoggedIn)
</script>

<style>
#navbar {
  --bs-primary-rgb: 42, 68, 96;
}
</style>
