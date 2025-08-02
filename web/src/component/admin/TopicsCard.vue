<template>
  <BCard>
    <template #header>
      <div class="d-flex justify-content-between align-items-center">
        <span>Topics</span>
        <div>
          <BButton
            size="sm"
            variant="primary"
            @click="showCreateModal"
            class="me-2"
          >
            Create Topic
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
      v-if="topics && topics.length > 0"
      :items="topics"
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
      <template #cell(name)="{ item }">
        <span
          v-b-tooltip.hover
          :title="item.id"
        >{{ item.name }}</span>
        <BButton
          size="sm"
          variant="link"
          @click="copyToClipboard(item.id, `topic-id-${item.id}`)"
        >
          <i class="bi bi-clipboard" />
        </BButton>
        <span v-if="copied === `topic-id-${item.id}`">Copied!</span>
      </template>
      <template #cell(code)="{ item }">
        <span>{{ item.code }}</span>
        <BButton
          size="sm"
          variant="link"
          @click="copyToClipboard(item.code, `topic-code-${item.id}`)"
        >
          <i class="bi bi-clipboard" />
        </BButton>
        <span v-if="copied === `topic-code-${item.id}`">Copied!</span>
      </template>
      <template #cell(owner_name)="{ item }">
        <span
          v-b-tooltip.hover
          :title="item.owner_id"
        >{{ item.owner_name }}</span>
      </template>
      <template #cell(latest_message_offset)="{ item }">
        <span>{{ item.latest_message_offset }}</span>
      </template>
      <template #cell(created_at)="{ item }">
        {{ new Date(item.created_at).toLocaleString() }}
      </template>
      <template #cell(actions)="{ item }">
        <div class="d-flex align-items-center">
          <BButton
            class="mx-1"
            size="sm"
            variant="info"
            @click="showSubscriptionsModal(item)"
          >
            Subscriptions
          </BButton>
          <BButton
            class="mx-1"
            size="sm"
            variant="info"
            @click="showUsersModal(item)"
          >
            Users
          </BButton>
          <BButton
            class="mx-1"
            size="sm"
            variant="danger"
            @click="showDeleteModal(item)"
          >
            Delete
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
      No topic found.
    </div>

    <hr>

    <AppPagination
      url="/v1/admin/topic"
      data-key="topics"
      @fetched="handleFetched"
      ref="paginationRef"
    />

    <!-- Topic Subscriptions Modal -->
    <BModal
      v-model="isSubscriptionsModalVisible"
      :title="`Subscriptions for Topic: ${selectedTopic?.name}`"
      @hidden="onSubscriptionsModalHidden"
      size="lg"
      hide-footer
    >
      <BTable
        :items="topicSubscriptions"
        :fields="topicSubscriptionsFields"
        striped
        hover
        responsive
      >
        <template #cell(endpoint_name)="{ item }">
          <router-link :to="{ name: (!item.owner_id || item.owner_id === state.user?.id) ? 'EndpointManager' : 'AdminEndpointManager', params: { id: item.endpoint_id } }">
            {{ item.endpoint_name }}
          </router-link>
        </template>
        <template #cell(owner_name)="{ item }">
          <span
            v-b-tooltip.hover
            :title="item.owner_id"
          >{{ item.owner_name }}</span>
        </template>
        <template #cell(created_at)="{ item }">
          {{ new Date(item.created_at).toLocaleString() }}
        </template>
      </BTable>
      <div
        v-if="topicSubscriptions.length === 0"
        class="text-center text-muted"
      >
        No subscriptions found for this topic.
      </div>
      <hr>
      <AppPagination
        :url="topicSubscriptionUrl"
        data-key="subscriptions"
        @fetched="handleSubscriptionFetched"
        ref="subscriptionPaginationRef"
      />
    </BModal>

    <!-- Topic Users Modal -->
    <BModal
      v-model="isUsersModalVisible"
      :title="`Users for Topic: ${selectedTopic?.name}`"
      @hidden="onUsersModalHidden"
      size="xl"
      hide-footer
    >
      <BTable
        :items="topicUsers"
        :fields="topicUsersFields"
        striped
        hover
        responsive
      >
        <template #cell(username)="{ item }">
          <span
            v-b-tooltip.hover
            :title="item.user_id"
          >{{ item.username }}</span>
        </template>
        <template #cell(permissions)="{ item }">
          <div class="topic-permission-grid">
            <BFormCheckbox
              v-for="p in basePermissions"
              :key="p.value"
              v-model="item.draft_permissions"
              :value="p.value"
              :disabled="item.user_id === selectedTopic?.owner_id"
              switches
            >
              {{ p.name }}
            </BFormCheckbox>
            <BFormCheckbox
              v-for="p in managePermissions"
              :key="p.value"
              v-model="item.draft_permissions"
              :value="p.value"
              :disabled="item.user_id === selectedTopic?.owner_id"
              switches
            >
              {{ p.name }}
            </BFormCheckbox>
          </div>
        </template>
        <template #cell(actions)="{ item }">
          <div
            v-if="item.user_id !== selectedTopic?.owner_id"
            class="d-flex align-items-center"
          >
            <BButton
              class="mx-1"
              size="sm"
              variant="primary"
              @click="saveTopicUserPermissions(item)"
            >
              Save
            </BButton>
            <BButton
              class="mx-1"
              size="sm"
              variant="outline-danger"
              @click="removeTopicUserPermissions(item)"
            >
              Remove
            </BButton>
          </div>
        </template>
      </BTable>
      <div
        v-if="topicUsers.length === 0"
        class="text-center text-muted"
      >
        No users have permission on this topic.
      </div>
      <AppPagination
        :url="topicUsersUrl"
        data-key="users"
        @fetched="handleTopicUsersFetched"
        ref="topicUsersPaginationRef"
      />

      <hr>

      <BForm @submit.prevent="assignTopicUserPermissions">
        <div class="row g-3 align-items-end">
          <BFormGroup
            label="Search User"
            label-for="topic-user-search-input"
            class="col-md-4"
          >
            <BFormInput
              id="topic-user-search-input"
              v-model="userPermissionSearchQuery"
              placeholder="Name or email prefix"
              debounce="300"
            />
          </BFormGroup>
          <BFormGroup
            v-if="isFetchingUsers"
            label-for="topic-user-select"
            class="col-md-4"
          >
            <template #label>
              <div
                class="spinner-border spinner-border-sm me-1"
                role="status"
              />
              <span>Searching User...</span>
            </template>
            <BFormSelect
              id="topic-user-select"
              v-model="newUserPermission.userId"
              :options="[]"
            >
              <template #first>
                <BFormSelectOption
                  :value="null"
                  disabled
                >
                  -- Select user --
                </BFormSelectOption>
              </template>
            </BFormSelect>
          </BFormGroup>
          <BFormGroup
            v-else
            label="User"
            label-for="topic-user-select"
            class="col-md-4"
          >
            <BFormSelect
              id="topic-user-select"
              v-model="newUserPermission.userId"
              :options="assignableUsers"
            >
              <template #first>
                <BFormSelectOption
                  :value="null"
                  disabled
                >
                  -- Select user --
                </BFormSelectOption>
              </template>
            </BFormSelect>
          </BFormGroup>
          <BFormGroup
            label="Permissions"
            label-for="topic-user-permission-checks"
            class="col-12"
          >
            <div
              id="topic-user-permission-checks"
              class="assign-topic-permission-grid"
            >
              <BFormCheckbox
                v-for="p in basePermissions"
                :key="p.value"
                v-model="newUserPermission.permissions"
                :value="p.value"
                switches
              >
                {{ p.name }}
              </BFormCheckbox>
              <BFormCheckbox
                v-for="p in managePermissions"
                :key="p.value"
                v-model="newUserPermission.permissions"
                :value="p.value"
                switches
              >
                {{ p.name }}
              </BFormCheckbox>
            </div>
          </BFormGroup>
        </div>
        <div class="mt-3 d-flex justify-content-end">
          <BButton
            type="submit"
            variant="primary"
            :disabled="!canAssignUserPermission"
          >
            Assign Permission
          </BButton>
        </div>
      </BForm>
    </BModal>

    <!-- Delete Topic Modal -->
    <BModal
      v-model="deleteModalShown"
      title="Delete Topic"
      ok-variant="danger"
      ok-title="Delete"
      @ok="deleteTopic"
    >
      <p>Are you sure you want to delete topic <strong>{{ selectedTopic ? selectedTopic.name : '' }}</strong> ({{ selectedTopic ? selectedTopic.id : '' }})?</p>
    </BModal>

    <!-- Create Topic Modal -->
    <BModal
      v-model="createModalShown"
      title="Create Topic"
      ok-title="Create"
      @ok="createTopic"
    >
      <form @submit.prevent="createTopic">
        <div class="mb-3">
          <label
            for="topicName"
            class="form-label"
          >Name</label>
          <input
            type="text"
            class="form-control"
            id="topicName"
            v-model="newTopic.name"
            required
            placeholder="Topic Name"
          >
        </div>
        <div class="mb-3">
          <label
            for="topicCode"
            class="form-label"
          >Code</label>
          <input
            type="text"
            class="form-control"
            id="topicCode"
            v-model="newTopic.code"
            required
            placeholder="topic_code"
          >
          <div class="form-text">
            Unique identifier for the topic.
          </div>
        </div>
        <div class="mb-3">
          <label
            for="topicDescription"
            class="form-label"
          >Description</label>
          <textarea
            class="form-control"
            id="topicDescription"
            v-model="newTopic.description"
            rows="3"
            placeholder="Optional description"
          />
        </div>
      </form>
    </BModal>
  </BCard>
</template>

<script setup>
import { computed, ref, watch } from 'vue'
import { BButton, BCard, BModal, BForm, BFormCheckbox, BFormGroup, BFormInput, BFormSelect, BFormSelectOption, BTable, vBTooltip } from 'bootstrap-vue-next'
import apiClient from '../../service/api'
import { useToast } from 'vue-toast-notification'
import AppPagination from '../AppPagination.vue'
import { state } from '../../service/state'
import { TopicPermissionType } from '../../service/enum'

const $toast = useToast()
const paginationRef = ref(null)
const topics = ref([])

const fields = [
  { key: 'show_details', label: '', tdClass: 'align-middle' },
  { key: 'name', label: 'Name', tdClass: 'align-middle' },
  { key: 'code', label: 'Code', tdClass: 'align-middle' },
  { key: 'owner_name', label: 'Owner', tdClass: 'align-middle' },
  { key: 'latest_message_offset', label: 'Latest Offset', tdClass: 'align-middle' },
  { key: 'created_at', label: 'Created At', tdClass: 'align-middle' },
  { key: 'actions', label: 'Actions', tdClass: 'align-middle' },
]

const handleFetched = (items) => {
  topics.value = items
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

const deleteModalShown = ref(false)
const selectedTopic = ref(null)

// Topic Subscriptions Modal
const isSubscriptionsModalVisible = ref(false)
const topicSubscriptions = ref([])
const subscriptionPaginationRef = ref(null)
const topicSubscriptionUrl = ref('')

const topicSubscriptionsFields = [
  { key: 'endpoint_name', label: 'Endpoint Name', tdClass: 'align-middle' },
  { key: 'endpoint_code', label: 'Endpoint Code', tdClass: 'align-middle' },
  { key: 'owner_name', label: 'Owner', tdClass: 'align-middle' },
  { key: 'committed_offset', label: 'Committed Offset', tdClass: 'align-middle' },
  { key: 'created_at', label: 'Subscribed At', tdClass: 'align-middle' },
]

const handleSubscriptionFetched = (items) => {
  topicSubscriptions.value = items
}

const showSubscriptionsModal = (topic) => {
  selectedTopic.value = topic
  topicSubscriptionUrl.value = `/v1/admin/topic/${topic.id}/subscription`
  isSubscriptionsModalVisible.value = true
}

const onSubscriptionsModalHidden = () => {
  selectedTopic.value = null
  topicSubscriptionUrl.value = ''
}

// Topic Users Modal
const isUsersModalVisible = ref(false)
const topicUsers = ref([])
const users = ref([])
const isFetchingUsers = ref(false)
const userPermissionSearchQuery = ref('')
const topicUsersUrl = ref("")
const newUserPermission = ref({
  userId: null,
  permissions: ['Read']
})
const topicUsersPaginationRef = ref(null)

const topicUsersFields = [
  { key: 'username', label: 'Username', tdClass: 'align-middle' },
  { key: 'email', label: 'Email', tdClass: 'align-middle' },
  { key: 'permissions', label: 'Permissions', tdClass: 'align-middle' },
  { key: 'actions', label: 'Actions', tdClass: 'align-middle' },
]

const allPermissions = Object.keys(TopicPermissionType).map(key => TopicPermissionType[key])

const basePermissions = allPermissions.filter(p => !p.value.startsWith('Manage'))
const managePermissions = allPermissions.filter(p => p.value.startsWith('Manage'))

const handleTopicUsersFetched = (items) => {
  topicUsers.value = items.map(user => ({
    ...user,
    draft_permissions: [...user.permissions]
  }))
}

const assignableUsers = computed(() => {
  const assignedUserIds = new Set(topicUsers.value.map(user => user.user_id))
  return users.value
    .filter(user => !assignedUserIds.has(user.id))
    .map(user => ({
      value: user.id,
      text: `${user.username} (${user.email})`
    }))
})

const canAssignUserPermission = computed(() => {
  return !!newUserPermission.value.userId && newUserPermission.value.permissions.length > 0
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
  } catch (error) {
    console.error('Failed to fetch users:', error)
  } finally {
    isFetchingUsers.value = false
  }
}

watch(userPermissionSearchQuery, (newVal) => {
  if (isUsersModalVisible.value) {
    fetchUsers(newVal)
  }
})

const showUsersModal = (topic) => {
  selectedTopic.value = topic
  topicUsers.value = []
  users.value = []
  userPermissionSearchQuery.value = ''
  newUserPermission.value = {
    userId: null,
    permissions: ['Read']
  }
  isUsersModalVisible.value = true
  topicUsersUrl.value = `/v1/admin/topic/${selectedTopic?.value.id}/user`
  topicUsersPaginationRef.value?.fetchData()
  fetchUsers()
}

const onUsersModalHidden = () => {
  selectedTopic.value = null
  topicUsers.value = []
  users.value = []
  userPermissionSearchQuery.value = ''
  topicUsersUrl.value = ''
}

async function saveTopicUserPermissions(user) {
  if (!selectedTopic.value) return
  try {
    await apiClient.put(`/v1/admin/topic/${selectedTopic.value.id}/user/${user.user_id}/permission`, {
      assign_new_user: false,
      permissions: user.draft_permissions
    })
    $toast.success('Topic user permissions updated successfully.')
    topicUsersPaginationRef.value?.fetchData()
  } catch (error) {
    console.error('Failed to update topic user permissions:', error)
    $toast.error('Failed to update topic user permissions: ' + (error.response?.data?.message || error.message))
  }
}

async function removeTopicUserPermissions(user) {
  if (!selectedTopic.value) return
  try {
    await apiClient.put(`/v1/admin/topic/${selectedTopic.value.id}/user/${user.user_id}/permission`, {
      assign_new_user: false,
      permissions: [],
    })
    $toast.success('Topic user permissions removed successfully.')
    topicUsersPaginationRef.value?.fetchData()
    fetchUsers(userPermissionSearchQuery.value)
  } catch (error) {
    console.error('Failed to remove topic user permissions:', error)
    $toast.error('Failed to remove topic user permissions: ' + (error.response?.data?.message || error.message))
  }
}

async function assignTopicUserPermissions() {
  if (!selectedTopic.value || !canAssignUserPermission.value) return
  try {
    await apiClient.put(`/v1/admin/topic/${selectedTopic.value.id}/user/${newUserPermission.value.userId}/permission`, {
      assign_new_user: true,
      permissions: newUserPermission.value.permissions
    })
    $toast.success('Topic user permissions assigned successfully.')
    newUserPermission.value = {
      userId: null,
      permissions: ['Read']
    }
    topicUsersPaginationRef.value?.fetchData()
    fetchUsers(userPermissionSearchQuery.value)
  } catch (error) {
    console.error('Failed to assign topic user permissions:', error)
    $toast.error('Failed to assign topic user permissions: ' + (error.response?.data?.message || error.message))
  }
}

const createModalShown = ref(false)
const newTopic = ref({
  name: '',
  code: '',
  description: ''
})

const showCreateModal = () => {
  newTopic.value = {
    name: '',
    code: '',
    description: ''
  }
  createModalShown.value = true
}

const createTopic = async (event) => {
  if (event && event.preventDefault) event.preventDefault()
  try {
    await apiClient.post('/v1/admin/topic', newTopic.value)
    $toast.success('Topic created successfully.')
    createModalShown.value = false
    paginationRef.value?.fetchData()
  } catch (error) {
    console.error('Failed to create topic:', error)
    $toast.error('Failed to create topic: ' + (error.response?.data?.message || error.message))
  }
}

const showDeleteModal = (topic) => {
  selectedTopic.value = topic
  deleteModalShown.value = true
}

const deleteTopic = async () => {
  try {
    await apiClient.delete(`/v1/admin/topic/${selectedTopic.value.id}`)
    $toast.success('Topic deleted successfully.')
    paginationRef.value?.fetchData()
  } catch (error) {
    console.error('Failed to delete topic:', error)
    $toast.error('Failed to delete topic: ' + (error.response?.data?.message || error.message))
  }
  deleteModalShown.value = false
}
</script>

<style scoped>
.topic-permission-grid {
  display: grid;
  grid-template-columns: repeat(3, minmax(8rem, 1fr));
  gap: 0.35rem 0.75rem;
  min-width: 40rem;
}

.assign-topic-permission-grid {
  display: grid;
  grid-template-columns: repeat(3, minmax(0, 1fr));
  gap: 0.35rem 0.75rem;
  width: 100%;
}

@media (max-width: 40rem) {
  .assign-topic-permission-grid {
    grid-template-columns: 1fr;
  }
}
</style>
