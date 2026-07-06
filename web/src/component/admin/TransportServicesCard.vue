<template>
  <BCard>
    <template #header>
      <div class="d-flex justify-content-between align-items-center">
        <span>Transport Services</span>
        <div class="d-flex gap-2">
          <BButton
            size="sm"
            variant="primary"
            @click="showCreateModal"
          >
            Create Transport Service
          </BButton>
          <BButton
            size="sm"
            variant="outline-primary"
            @click="paginationRef?.fetchData()"
          >
            Refresh
          </BButton>
        </div>
      </div>
    </template>
    <BTable
      v-if="services && services.length > 0"
      :items="services"
      :fields="fields"
      striped
      responsive
    >
      <template #cell(show_details)="row">
        <BButton
          size="sm"
          variant="outline-secondary"
          @click="row.toggleDetails"
          class="px-2 py-0"
        >
          {{ row.detailsShowing ? '-' : '+' }}
        </BButton>
      </template>
      <template #cell(id)="{ item }">
        <span>{{ item.id }}</span>
        <BButton
          size="sm"
          variant="link"
          @click="copyToClipboard(item.id, `service-id-${item.id}`)"
        >
          <i class="bi bi-clipboard" />
        </BButton>
        <span v-if="copied === `service-id-${item.id}`">Copied!</span>
      </template>
      <template #cell(transport_service_type)="{ item }">
        {{ TransportServiceType[item.transport_service_type]?.name }}
      </template>
      <template #cell(created_at)="{ item }">
        {{ new Date(item.created_at).toLocaleString() }}
      </template>
      <template #cell(actions)="{ item }">
        <div class="d-flex align-items-center">
          <BButton
            class="mx-1"
            size="sm"
            :variant="item.enabled ? 'danger' : 'success'"
            @click="showToggleModal(item)"
          >
            {{ item.enabled ? 'Disable' : 'Enable' }}
          </BButton>
        </div>
      </template>
      <template #row-details="row">
        <BCard>
          <div
            v-if="row.item.description"
            style="white-space: pre-wrap;"
          >
            <strong>Description:</strong><br>
            {{ row.item.description }}
          </div>
          <div
            v-else
            class="text-muted italic"
          >
            No description provided.
          </div>
        </BCard>
      </template>
    </BTable>
    <div
      v-else
      class="text-center text-muted"
    >
      No transport service found.
    </div>

    <hr>

    <AppPagination
      url="/v1/admin/transport-service"
      data-key="transport_services"
      @fetched="handleFetched"
      ref="paginationRef"
    />

    <!-- Toggle Transport Service Enabled Modal -->
    <BModal
      v-model="toggleModalShown"
      :title="selectedService?.enabled ? 'Disable Transport Service' : 'Enable Transport Service'"
      :ok-variant="selectedService?.enabled ? 'danger' : 'success'"
      :ok-title="selectedService?.enabled ? 'Disable' : 'Enable'"
      @ok="toggleServiceEnabled"
    >
      <p>
        Are you sure you want to {{ selectedService?.enabled ? 'disable' : 'enable' }} transport service
        <strong>{{ selectedService ? selectedService.name : '' }}</strong> ({{ selectedService ? selectedService.id : '' }})?
      </p>
    </BModal>

    <!-- Create Transport Service Modal -->
    <BModal
      v-model="createModalShown"
      title="Create Transport Service"
      @ok="handleCreateService"
      @hidden="resetCreateModal"
      ok-title="Create"
      :ok-disabled="!isCreateFormValid"
    >
      <BForm @submit.prevent="handleCreateService">
        <BFormGroup
          label="Service Type"
          label-for="service-type-select"
          class="mb-3"
        >
          <BFormSelect
            id="service-type-select"
            v-model="newService.type"
            :options="typeOptions"
            required
          />
        </BFormGroup>

        <BFormGroup
          label="Name"
          label-for="service-name-input"
          class="mb-3"
        >
          <BFormInput
            id="service-name-input"
            v-model="newService.name"
            required
            placeholder="Enter service name"
          />
        </BFormGroup>

        <BFormGroup
          label="Description"
          label-for="service-description-textarea"
          class="mb-3"
        >
          <BFormTextarea
            id="service-description-textarea"
            v-model="newService.description"
            placeholder="Enter optional description"
            rows="3"
          />
        </BFormGroup>

        <!-- Telegram Specific Fields -->
        <div v-if="newService.type === telegramTypeCode">
          <BFormGroup
            label="Bot Token"
            label-for="service-token-input"
            class="mb-3"
          >
            <BFormInput
              id="service-token-input"
              v-model="newService.token"
              required
              type="password"
              placeholder="Enter Telegram Bot Token"
            />
          </BFormGroup>

          <BFormGroup
            label="Assign to User"
            label-for="user-assignment-select"
            class="mb-3"
          >
            <BFormSelect
              id="user-assignment-select"
              v-model="newService.assignmentType"
              :options="assignmentOptions"
              required
            />
          </BFormGroup>

          <div v-if="newService.assignmentType === 'new'">
            <BFormGroup
              label="User Name"
              label-for="new-user-name"
              class="mb-3"
            >
              <BFormInput
                id="new-user-name"
                v-model="newService.newUserName"
                required
                placeholder="User Name"
              />
            </BFormGroup>
            <BFormGroup
              label="User Email"
              label-for="new-user-email"
              class="mb-3"
            >
              <BFormInput
                id="new-user-email"
                v-model="newService.newUserEmail"
                type="email"
                required
                placeholder="User Email"
              />
            </BFormGroup>
          </div>
          <div v-else-if="newService.assignmentType === 'existing'">
            <BFormGroup
              label="Search User (Name/Email prefix)"
              label-for="user-search-input"
              class="mb-2"
            >
              <BFormInput
                id="user-search-input"
                v-model="userSearchQuery"
                placeholder="Type to search..."
                debounce="300"
              />
            </BFormGroup>
            <BFormGroup
              label="Select User"
              label-for="user-select"
              class="mb-3"
            >
              <BFormSelect
                id="user-select"
                v-model="newService.userId"
                :options="userOptions"
                required
              >
                <template #first>
                  <BFormSelectOption
                    :value="null"
                    disabled
                  >
                    -- Please select a user --
                  </BFormSelectOption>
                </template>
              </BFormSelect>
              <div
                v-if="isFetchingUsers"
                class="mt-1 small text-muted"
              >
                <div
                  class="spinner-border spinner-border-sm me-1"
                  role="status"
                />
                Searching...
              </div>
            </BFormGroup>
          </div>
          <div
            v-else-if="newService.assignmentType === 'myself'"
            class="mb-3 text-info"
          >
            Service will be bound to your current account.
          </div>
        </div>
      </BForm>
    </BModal>

    <!-- Telegram Setup Token Modal -->
    <BModal
      v-model="setupTokenModalShown"
      title="Telegram Service Setup"
      ok-only
      ok-title="Close"
      @hidden="onSetupTokenModalHidden"
    >
      <p>The Telegram service is being initialized. Please send your bot token to the bot on Telegram to complete the setup.</p>
      <strong>Setup Token:</strong>
      <BAlert
        show
        variant="info"
        body-class="d-flex align-items-center container-lg"
      >
        <div class="text-truncate">
          {{ setupToken }}
        </div>
        <div class="ms-auto">
          <BButton
            size="sm"
            variant="outline-secondary"
            @click="copyToClipboard(setupToken, 'setup-token')"
          >
            <i class="bi bi-clipboard" />
          </BButton>
        </div>
      </BAlert>
      <span
        v-if="copied === 'setup-token'"
        class="text-success"
      >Copied!</span>
      <p class="text-muted mt-3">
        This dialog will close automatically once the setup is confirmed in Telegram.
      </p>
      <p class="text-muted small">
        This setup request will time out in {{ Math.floor(timeout / 1000) }} seconds.
      </p>
    </BModal>
  </BCard>
</template>

<script setup>
import { ref, computed, watch, onMounted, onBeforeUnmount } from 'vue'
import { BCard, BTable, BButton, BModal, BForm, BFormGroup, BFormInput, BFormSelect, BFormSelectOption, BFormTextarea, BAlert } from 'bootstrap-vue-next'
import apiClient from '../../service/api'
import { useToast } from 'vue-toast-notification'
import AppPagination from '../AppPagination.vue'
import { TransportServiceType } from '../../service/enum'

const $toast = useToast()
const paginationRef = ref(null)
const services = ref([])
const users = ref([])
const isFetchingUsers = ref(false)
const userSearchQuery = ref('')

const fields = [
  { key: 'show_details', label: '', tdClass: 'align-middle' },
  { key: 'id', label: 'ID', tdClass: 'align-middle' },
  { key: 'transport_service_type', label: 'Type', tdClass: 'align-middle' },
  { key: 'name', label: 'Name', tdClass: 'align-middle' },
  { key: 'created_at', label: 'Created At', tdClass: 'align-middle' },
  { key: 'actions', label: 'Actions', tdClass: 'align-middle' },
]

const handleFetched = (items) => {
  services.value = items
}

const copied = ref(null)
const copyToClipboard = (text, type) => {
  navigator.clipboard.writeText(text).then(() => {
    copied.value = type
    setTimeout(() => {
      copied.value = null
    }, 2000)
  })
}

const toggleModalShown = ref(false)
const selectedService = ref(null)

const showToggleModal = (service) => {
  selectedService.value = service
  toggleModalShown.value = true
}

const toggleServiceEnabled = async () => {
  try {
    const newStatus = !selectedService.value.enabled
    await apiClient.put(`/v1/admin/transport-service/${selectedService.value.id}/enabled`, newStatus)
    $toast.success(`Transport service ${newStatus ? 'enabled' : 'disabled'} successfully.`)
    paginationRef.value?.fetchData()
  } catch (error) {
    console.error('Failed to toggle transport service status:', error)
    $toast.error('Failed to toggle transport service status: ' + (error.response?.data?.message || error.message))
  }
  toggleModalShown.value = false
}

// Create Transport Service
const httpTypeCode = Number(Object.keys(TransportServiceType).find(k => TransportServiceType[k].code === 'HTTP'))
const telegramTypeCode = Number(Object.keys(TransportServiceType).find(k => TransportServiceType[k].code === 'TELEGRAM'))

const createModalShown = ref(false)
const setupTokenModalShown = ref(false)
const setupToken = ref('')

const POLLING_INTERVAL = 3000 // 3 seconds
const POLLING_TIMEOUT = 300000 // 5 minutes

const pollingId = ref(null)
const timeoutId = ref(null)
const timeout = ref(POLLING_TIMEOUT)
const countdownIntervalId = ref(null)

const startPolling = (token) => {
  stopPolling()
  timeout.value = POLLING_TIMEOUT

  pollingId.value = setInterval(async () => {
    try {
      const response = await apiClient.get('/v1/admin/transport-service/telegram/setup', {
        params: { token },
      })
      if (response.data.data.created === null) {
        stopPolling()
        localStorage.removeItem('pendingTelegramSetupToken')
        setupTokenModalShown.value = false
        $toast.success('The creation of Telegram transport service is timeout!')
        paginationRef.value?.fetchData()
      } else if (response.data.data.created) {
        stopPolling()
        localStorage.removeItem('pendingTelegramSetupToken')
        setupTokenModalShown.value = false
        $toast.success('Telegram transport service successfully created!')
        paginationRef.value?.fetchData()
      }
    } catch (error) {
      if (error.response?.status !== 404) {
        console.error('Polling error:', error)
      }
    }
  }, POLLING_INTERVAL)

  timeoutId.value = setTimeout(() => {
    stopPolling()
    localStorage.removeItem('pendingTelegramSetupToken')
    setupTokenModalShown.value = false
    $toast.error('Telegram service setup timed out. Please try again.')
  }, POLLING_TIMEOUT)

  countdownIntervalId.value = setInterval(() => {
    timeout.value -= 1000
    if (timeout.value <= 0) {
      clearInterval(countdownIntervalId.value)
    }
  }, 1000)
}

const stopPolling = () => {
  if (pollingId.value) {
    clearInterval(pollingId.value)
    pollingId.value = null
  }
  if (timeoutId.value) {
    clearTimeout(timeoutId.value)
    timeoutId.value = null
  }
  if (countdownIntervalId.value) {
    clearInterval(countdownIntervalId.value)
    countdownIntervalId.value = null
  }
}

const onSetupTokenModalHidden = () => {
  stopPolling()
  localStorage.removeItem('pendingTelegramSetupToken')
}

onMounted(() => {
  const pendingToken = localStorage.getItem('pendingTelegramSetupToken')
  if (pendingToken) {
    setupToken.value = pendingToken
    setupTokenModalShown.value = true
    startPolling(pendingToken)
  }
})

onBeforeUnmount(() => {
  stopPolling()
})

const newService = ref({
  type: httpTypeCode,
  name: '',
  description: '',
  token: '',
  assignmentType: 'myself', // 'myself', 'existing', 'new'
  userId: null,
  newUserName: '',
  newUserEmail: ''
})

const typeOptions = Object.keys(TransportServiceType).map(key => ({
  value: Number(key),
  text: TransportServiceType[key].name
}))

const assignmentOptions = [
  { value: 'myself', text: 'Myself' },
  { value: 'existing', text: 'Existing User' },
  { value: 'new', text: 'New User' }
]

const userOptions = computed(() => {
  return users.value.map(u => ({
    value: u.id,
    text: `${u.username} (${u.email})`
  }))
})

const isCreateFormValid = computed(() => {
  if (!newService.value.name || newService.value.name.length < 4) return false

  if (newService.value.type === httpTypeCode) {
    return true
  }

  if (newService.value.type === telegramTypeCode) {
    if (!newService.value.token) return false
    if (newService.value.assignmentType === 'new') {
      return newService.value.newUserName.length >= 4 && /^\S+@\S+\.\S+$/.test(newService.value.newUserEmail)
    } else if (newService.value.assignmentType === 'existing') {
      return !!newService.value.userId
    } else {
      return true // 'myself' is always valid
    }
  }

  return false
})

async function fetchUsers(search = '') {
  isFetchingUsers.value = true
  try {
    const params = { page: 1, size: 10 }
    if (search) {
      params.search = search
    }
    const response = await apiClient.get('/v1/admin/user', { params })
    users.value = response.data.data.users
    // Reset selection if current selection is not in the results and search is active
    if (search && newService.value.userId && !users.value.find(u => u.id === newService.value.userId)) {
        // We keep it for now to avoid UX annoyance, or we could reset it.
        // Better UX is to keep it if it was already selected.
    }
  } catch (error) {
    console.error('Failed to fetch users:', error)
  } finally {
    isFetchingUsers.value = false
  }
}

watch(userSearchQuery, (newVal) => {
  fetchUsers(newVal)
})

const showCreateModal = () => {
  userSearchQuery.value = ''
  fetchUsers()
  createModalShown.value = true
}

const resetCreateModal = () => {
  newService.value = {
    type: telegramTypeCode,
    name: '',
    description: '',
    token: '',
    assignmentType: 'myself',
    userId: null,
    newUserName: '',
    newUserEmail: ''
  }
}

const handleCreateService = async (event) => {
  if (event && event.preventDefault) event.preventDefault()

  try {
    if (newService.value.type === httpTypeCode) {
      await apiClient.post('/v1/admin/transport-service/http', {
        name: newService.value.name,
        description: newService.value.description || null
      })
      $toast.success('HTTP transport service created successfully.')
      createModalShown.value = false
      paginationRef.value?.fetchData()
    } else if (newService.value.type === telegramTypeCode) {
      const payload = {
        name: newService.value.name,
        description: newService.value.description || null,
        token: newService.value.token,
      }
      if (newService.value.assignmentType === 'new') {
        payload.new_user = {
          name: newService.value.newUserName,
          email: newService.value.newUserEmail
        }
      } else if (newService.value.assignmentType === 'existing') {
        payload.user_id = newService.value.userId
      }

      const res = await apiClient.post('/v1/admin/transport-service/telegram', payload)
      setupToken.value = res.data.data.setup_token
      localStorage.setItem('pendingTelegramSetupToken', setupToken.value)
      startPolling(setupToken.value)
      $toast.success('Telegram transport service setup initiated.')
      createModalShown.value = false
      setupTokenModalShown.value = true
      paginationRef.value?.fetchData()
    }
  } catch (error) {
    console.error('Failed to create transport service:', error)
    $toast.error('Failed to create transport service: ' + (error.response?.data?.message || error.message))
  }
}
</script>
