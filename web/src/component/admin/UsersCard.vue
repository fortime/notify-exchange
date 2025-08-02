<template>
  <BCard>
    <template #header>
      <div class="d-flex justify-content-between align-items-center">
        <div class="d-flex align-items-center gap-3">
          <span>Users</span>
          <BFormInput
            v-model="searchQuery"
            size="sm"
            placeholder="Search name/email..."
            debounce="300"
            style="width: 250px;"
          />
        </div>
        <BButton
          size="sm"
          variant="outline-primary"
          @click="paginationRef?.fetchData()"
        >
          Refresh
        </BButton>
      </div>
    </template>
    <BTable
      v-if="users && users.length > 0"
      :items="users"
      :fields="fields"
      striped
      responsive
    >
      <template #cell(id)="{ item }">
        <span>{{ item.id }}</span>
        <BButton
          size="sm"
          variant="link"
          @click="copyToClipboard(item.id, `user-id-${item.id}`)"
        >
          <i class="bi bi-clipboard" />
        </BButton>
        <span v-if="copied === `user-id-${item.id}`">Copied!</span>
      </template>
      <template #cell(email)="{ item }">
        <span>{{ item.email }}</span>
        <BButton
          size="sm"
          variant="link"
          @click="copyToClipboard(item.email, `user-email-${item.id}`)"
        >
          <i class="bi bi-clipboard" />
        </BButton>
        <span v-if="copied === `user-email-${item.id}`">Copied!</span>
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
    </BTable>
    <div
      v-else
      class="text-center text-muted"
    >
      No user found.
    </div>

    <hr>

    <AppPagination
      url="/v1/admin/user"
      data-key="users"
      :extra-params="searchParams"
      @fetched="handleFetched"
      ref="paginationRef"
    />

    <!-- Toggle User Enabled Modal -->
    <BModal
      v-model="toggleModalShown"
      :title="selectedUser?.enabled ? 'Disable User' : 'Enable User'"
      :ok-variant="selectedUser?.enabled ? 'danger' : 'success'"
      :ok-title="selectedUser?.enabled ? 'Disable' : 'Enable'"
      @ok="toggleUserEnabled"
    >
      <p>
        Are you sure you want to {{ selectedUser?.enabled ? 'disable' : 'enable' }} user
        <strong>{{ selectedUser ? selectedUser.username : '' }}</strong> ({{ selectedUser ? selectedUser.id : '' }})?
      </p>
    </BModal>
  </BCard>
</template>

<script setup>
import { ref, computed } from 'vue'
import { BCard, BTable, BButton, BModal, BFormInput } from 'bootstrap-vue-next'
import apiClient from '../../service/api'
import { useToast } from 'vue-toast-notification'
import AppPagination from '../AppPagination.vue'

const $toast = useToast()
const paginationRef = ref(null)
const users = ref([])
const searchQuery = ref('')

const searchParams = computed(() => {
  return searchQuery.value ? { search: searchQuery.value } : {}
})

const fields = [
  { key: 'id', label: 'ID', tdClass: 'align-middle' },
  { key: 'username', label: 'Username', tdClass: 'align-middle' },
  { key: 'email', label: 'Email', tdClass: 'align-middle' },
  { key: 'created_at', label: 'Created At', tdClass: 'align-middle' },
  { key: 'actions', label: 'Actions', tdClass: 'align-middle' },
]

const handleFetched = (items) => {
  users.value = items
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
const selectedUser = ref(null)

const showToggleModal = (user) => {
  selectedUser.value = user
  toggleModalShown.value = true
}

const toggleUserEnabled = async () => {
  try {
    const newStatus = !selectedUser.value.enabled
    await apiClient.put(`/v1/admin/user/${selectedUser.value.id}/enabled`, newStatus)
    $toast.success(`User ${newStatus ? 'enabled' : 'disabled'} successfully.`)
    paginationRef.value?.fetchData()
  } catch (error) {
    console.error('Failed to toggle user status:', error)
    $toast.error('Failed to toggle user status: ' + (error.response?.data?.message || error.message))
  }
  toggleModalShown.value = false
}
</script>
