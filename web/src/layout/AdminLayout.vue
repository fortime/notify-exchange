<template>
  <BApp>
    <BNavbar
      id="admin-navbar"
      v-b-color-mode="'dark'"
      variant="dark"
      toggleable="lg"
      container="lg"
    >
      <BNavbarBrand :to="{ name: 'AdminPortal' }">
        Notify Exchange Admin Portal
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
        <BNavbarNav class="mb-2 mb-lg-0">
          <BNavItem :to="{ name: 'Home' }">
            User Portal
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
#admin-navbar {
  --bs-dark-rgb: 150, 68, 96;
}
</style>
