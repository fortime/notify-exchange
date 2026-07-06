import { createRouter, createWebHistory } from 'vue-router'
import AppSetup from '../view/AppSetup.vue'
import LoginTelegram from '../view/LoginTelegram.vue'
import AppLogin from '../view/AppLogin.vue'
import AppHome from '../view/AppHome.vue'
import AdminPortal from '../view/AdminPortal.vue'
import AdminEndpointManager from '../view/AdminEndpointManager.vue'
import LogoutTelegram from '../view/LogoutTelegram.vue'
import EndpointManager from '../view/EndpointManager.vue'
import TopicMessages from '../view/TopicMessages.vue'
import MainLayout from '../layout/MainLayout.vue'
import AdminLayout from '../layout/AdminLayout.vue'
import { state } from '../service/state'
import apiClient from '../service/api'

const routes = [
  {
    path: '/',
    component: MainLayout,
    children: [
      {
        path: '',
        redirect: '/setup'
      },
      {
        path: 'setup',
        name: 'Setup',
        component: AppSetup
      },
      {
        path: 'login',
        name: 'Login',
        component: AppLogin,
      },
      {
        path: 'login/telegram',
        name: 'LoginTelegram',
        component: LoginTelegram,
      },
      {
        path: 'home',
        name: 'Home',
        component: AppHome
      },
      {
        path: 'logout/telegram',
        name: 'LogoutTelegram',
        component: LogoutTelegram
      },
      {
        path: 'endpoint/:id',
        name: 'EndpointManager',
        component: EndpointManager
      },
      {
        path: 'topic/:topicId/message',
        name: 'TopicMessages',
        component: TopicMessages,
        props: true
      }
    ]
  },
  {
    path: '/admin',
    component: AdminLayout,
    beforeEnter: (to, from, next) => {
      if (state.isLoggedIn && state.user?.is_admin) {
        next()
      } else {
        next({ name: 'Home' })
      }
    },
    children: [
      {
        path: '',
        name: 'AdminPortal',
        component: AdminPortal
      },
      {
        path: 'endpoint/:id',
        name: 'AdminEndpointManager',
        component: AdminEndpointManager
      }
    ]
  }
]

const router = createRouter({
  history: createWebHistory(process.env.BASE_URL),
  routes
})

router.beforeEach(async (to, from) => {
  // Run initialization checks if needed
  if (state.isInitialized === null || state.isInitialized === false) {
    try {
      await apiClient.get('/v1/initialized')
      state.isInitialized = true
    } catch (error) {
      if (error.response && error.response.status === 404) {
        state.isInitialized = false
      } else {
        console.error('Error during initialization check:', error)
        return false
      }
    }
  }

  // Define public routes that are always accessible and without fetching user info
  const publicPagesBefore = ['/logout/telegram']
  console.log("from " + from.path + ", to " + to.path)
  if (publicPagesBefore.includes(to.path)) {
    // Return before getting the user info.
    return true
  }

  // Check user login state if system is initialized
  if (state.isInitialized && state.user === null) { // if user is null, means we haven't checked or it failed
    try {
      const response = await apiClient.get('/v1/user/me')
      state.user = response.data.data
      state.isLoggedIn = true
    } catch (error) {
      if (!error.response || error.response.status !== 401) {
        console.error('Error fetching user status:', error)
      }
      const logoutRoute = localStorage.getItem('logoutRouteName')
      state.user = null
      state.isLoggedIn = false
      localStorage.removeItem('logoutRouteName')
      if (logoutRoute) {
        return { name: logoutRoute }
      }
    }
  }

  // Define public routes that are always accessible
  const publicPages = ['/login', '/login/telegram']
  if (publicPages.includes(to.path)) {
    return true
  }

  // 4. Handle routing based on state
  if (!state.isInitialized) {
    // If not initialized, only allow access to /setup
    if (to.path !== '/setup') {
      return '/setup'
    }
  } else { // System is initialized
    if (!state.isLoggedIn) {
      // If initialized but not logged in, redirect to login
      // This path is already handled by publicPages check, but as a catch-all it's good
      return { path: '/login', query: { ru: to.fullPath } }
    } else { // Initialized and logged in
      // If initialized and logged in, don't show setup
      if (to.path === '/setup') {
        return '/home'
      }
    }
  }

  // 5. If no other rule matched, let the user proceed
  return true
})

router.afterEach((to, from) => {
  // Keep track of navigation history in sessionStorage, excluding login pages
  if (from.name && from.name !== 'Login' && from.name !== 'LoginTelegram') {
    let history = JSON.parse(sessionStorage.getItem('navigationHistory') || '[]')
    history.push({ name: from.name, params: from.params, query: from.query })
    sessionStorage.setItem('navigationHistory', JSON.stringify(history))
  }
})

export default router
