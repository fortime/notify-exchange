<template>
  <BCard>
    <template #header>
      <div class="d-flex justify-content-between align-items-center">
        <span>My Topics</span>
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
      v-if="topics && topics.length > 0"
      :items="topics"
      :fields="fields"
      striped
      hover
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
      <template #cell(subscribed)="{ item }">
        <i
          v-if="item.subscribed"
          class="bi bi-check-circle-fill text-success"
        />
      </template>
      <template #cell(latest_message_offset)="{ item }">
        <router-link
          v-if="item.latest_message_offset !== 0 && item.permissions.some(p => p === TopicPermissionType['READ'].value)"
          :to="{ name: 'TopicMessages', params: { topicId: item.id }, query: { mo: item.latest_message_offset + 1 } }"
          class="text-primary text-decoration-underline"
        >
          {{ item.latest_message_offset }}
        </router-link>
        <span v-else>{{ item.latest_message_offset }}</span>
      </template>
      <template #cell(created_at)="{ item }">
        {{ new Date(item.created_at).toLocaleString() }}
      </template>
      <template #cell(actions)="{ item }">
        <div class="d-flex align-items-center">
          <BButton
            v-if="item.permissions.some(p => p === TopicPermissionType['READ'].value)"
            class="mx-1"
            size="sm"
            variant="success"
            @click="showSubscribeModal(item)"
          >
            Subscribe
          </BButton>
          <BButton
            v-if="item.subscribed"
            class="mx-1"
            size="sm"
            variant="info"
            @click="showSubscriptionsModal(item)"
          >
            Subscriptions
          </BButton>
          <BButton
            v-if="item.permissions.some(p => managePermissions.some(mp => mp.value == p))"
            class="mx-1"
            size="sm"
            variant="info"
            @click="showUsersModal(item)"
          >
            Users
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
      No topics have been created yet.
    </div>

    <hr>

    <AppPagination
      url="/v1/topic"
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
          <router-link :to="{ name: 'EndpointManager', params: { id: item.endpoint_id } }">
            {{ item.endpoint_name }}
          </router-link>
        </template>
        <template #cell(created_at)="{ item }">
          {{ new Date(item.created_at).toLocaleString() }}
        </template>
        <template #cell(actions)="{ item }">
          <div class="d-flex align-items-center">
            <BButton
              size="sm"
              variant="danger"
              @click="showDeleteSubscriptionModal(item)"
            >
              Unsubscribe
            </BButton>
          </div>
        </template>
      </BTable>
      <div
        v-if="topicSubscriptions.length === 0"
        class="text-center text-muted"
      >
        No subscriptions found for this topic.
      </div>
      <hr>
      <!-- url="`/v1/topic/${selectedTopic?.id}/subscription`" -->
      <AppPagination
        :url="topicSubscriptionUrl"
        data-key="subscriptions"
        @fetched="handleSubscriptionFetched"
        ref="subscriptionPaginationRef"
      />
    </BModal>

    <!-- Delete Subscription Modal -->
    <BModal
      v-model="isDeleteSubscriptionModalVisible"
      title="Confirm Unsubscribe"
      @ok="handleDeleteSubscription"
      ok-title="Unsubscribe"
      @hidden="resetDeleteSubscriptionModal"
      ok-variant="danger"
    >
      <p>Are you sure you want to unsubscribe endpoint <strong>{{ subscriptionToDelete?.endpoint_name }}</strong> from this topic?</p>
    </BModal>

    <!-- Subscribe to Topic Modal -->
    <BModal
      v-model="isSubscribeModalVisible"
      :title="`Subscribe to Topic: ${topicToSubscribe?.name}`"
      @ok="handleSubscribe"
      @hidden="resetSubscribeModal"
      ok-title="Subscribe"
    >
      <BFormGroup
        label="Endpoint ID"
        label-for="endpoint-id-input"
      >
        <BFormInput
          id="endpoint-id-input"
          v-model="endpointIdToSubscribe"
          placeholder="Enter the ID of the endpoint to subscribe"
          required
        />
      </BFormGroup>
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
              :disabled="!canChangePermission(selectedTopic, item.user_id, p)"
              switches
            >
              {{ p.name }}
            </BFormCheckbox>
            <BFormCheckbox
              v-for="p in managePermissions"
              :key="p.value"
              v-model="item.draft_permissions"
              :value="p.value"
              :disabled="!canChangePermission(selectedTopic, item.user_id, p)"
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
            label="User Id"
            label-for="assign-user-id"
            class="col-md-4"
          >
            <BFormInput
              id="assign-user-id"
              v-model="assignUserId"
              placeholder="User Id"
              debounce="300"
            />
          </BFormGroup>
          <BFormGroup
            label="Permissions"
            label-for="assign-user-permission-checks"
            class="col-12"
          >
            <div
              id="assign-user-permission-checks"
              class="assign-topic-permission-grid"
            >
              <BFormCheckbox
                v-for="p in basePermissions"
                :key="p.value"
                v-model="newUserPermission.permissions"
                :value="p.value"
                :disabled="!hasManagePermissionOf(selectedTopic?.permissions, p)"
                switches
              >
                {{ p.name }}
              </BFormCheckbox>
              <BFormCheckbox
                v-for="p in managePermissions"
                :key="p.value"
                v-model="newUserPermission.permissions"
                :value="p.value"
                :disabled="!hasManagePermissionOf(selectedTopic?.permissions, p)"
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
  </BCard>
</template>

<script setup>
import { computed, ref } from 'vue'
import { BButton, BCard, BForm, BFormCheckbox, BFormGroup, BFormInput, BModal, BTable, vBTooltip } from 'bootstrap-vue-next'
import apiClient from '../../service/api'
import { useToast } from 'vue-toast-notification'
import AppPagination from '../AppPagination.vue'
import { state } from '../../service/state'
import { TopicPermissionType } from '../../service/enum'

const allPermissions = Object.keys(TopicPermissionType).map(key => TopicPermissionType[key])

const basePermissions = allPermissions.filter(p => !p.value.startsWith('Manage'))
const managePermissions = allPermissions.filter(p => p.value.startsWith('Manage'))

const $toast = useToast()
const paginationRef = ref(null)
const topics = ref([])

const fields = [
  { key: 'show_details', label: '', tdClass: 'align-middle' },
  { key: 'name', label: 'Topic Name', tdClass: 'align-middle' },
  { key: 'code', label: 'Topic Code', tdClass: 'align-middle' },
  { key: 'owner_name', label: 'Owner', tdClass: 'align-middle' },
  { key: 'subscribed', label: 'Subscribed', tdClass: 'align-middle' },
  { key: 'latest_message_offset', label: 'Latest Offset', tdClass: 'align-middle' },
  { key: 'created_at', label: 'Created At', tdClass: 'align-middle' },
  { key: 'actions', label: 'Actions', tdClass: 'align-middle' }
]

const handleFetched = (items) => {
  topics.value = items.map(item => ({
    ...item[0],
    permissions: item[1],
  }))
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

// Topic Subscriptions Modal
const isSubscriptionsModalVisible = ref(false)
const selectedTopic = ref(null)
const topicSubscriptions = ref([])
const subscriptionPaginationRef = ref(null)
const topicSubscriptionUrl = ref("")

const handleSubscriptionFetched = (items) => {
  topicSubscriptions.value = items
}

const topicSubscriptionsFields = [
  { key: 'endpoint_name', label: 'Endpoint Name', tdClass: 'align-middle' },
  { key: 'endpoint_code', label: 'Endpoint Code', tdClass: 'align-middle' },
  { key: 'committed_offset', label: 'Committed Offset', tdClass: 'align-middle' },
  { key: 'created_at', label: 'Subscribed At', tdClass: 'align-middle' },
  { key: 'actions', label: 'Actions', tdClass: 'align-middle' },
]

async function showSubscriptionsModal(topic) {
  selectedTopic.value = topic
  isSubscriptionsModalVisible.value = true
  topicSubscriptionUrl.value = `/v1/topic/${selectedTopic.value.id}/subscription`
}

async function onSubscriptionsModalHidden(topic) {
  selectedTopic.value = null
  topicSubscriptionUrl.value = ''
}

// Delete Subscription Modal
const isDeleteSubscriptionModalVisible = ref(false)
const subscriptionToDelete = ref(null)

const showDeleteSubscriptionModal = (subscription) => {
  subscriptionToDelete.value = { ...subscription }
  isDeleteSubscriptionModalVisible.value = true
}

const handleDeleteSubscription = async () => {
  const subscription = subscriptionToDelete.value
  const topic = selectedTopic.value
  if (subscription) {
    try {
      await apiClient.delete(`/v1/endpoint/${subscription.endpoint_id}/subscription/${subscription.id}`)
      $toast.success(`Unsubscribed from topic '${topic.name}'.`)
      subscriptionPaginationRef.value?.fetchData()
    } catch (error) {
      console.error('Failed to unsubscribe:', error)
      $toast.error('Failed to unsubscribe: ' + (error.response?.data?.message || error.message))
    } finally {
      isDeleteSubscriptionModalVisible.value = false
    }
  }
}

const resetDeleteSubscriptionModal = () => {
  subscriptionToDelete.value = null
}

// Subscribe to Topic Modal
const isSubscribeModalVisible = ref(false)
const topicToSubscribe = ref(null)
const endpointIdToSubscribe = ref('')

function showSubscribeModal(topic) {
  topicToSubscribe.value = topic
  isSubscribeModalVisible.value = true
}

function resetSubscribeModal() {
  topicToSubscribe.value = null
  endpointIdToSubscribe.value = ''
}

async function handleSubscribe() {
  const topic = topicToSubscribe.value
  const endpointId = endpointIdToSubscribe.value
  if (!endpointId || !topic) return

  try {
    await apiClient.post(`/v1/topic/${topic.id}/subscription`, {
      endpoint_id: endpointId
    })
    $toast.success(`Successfully subscribed endpoint to topic '${topic.name}'.`)
    paginationRef.value?.fetchData()
  } catch (error) {
    console.error('Failed to subscribe:', error)
    $toast.error('Failed to subscribe: ' + (error.response?.data?.message || error.message))
  }
}

// Topic Users Modal
const isUsersModalVisible = ref(false)
const topicUsers = ref([])
const assignUserId = ref('')
const topicUsersUrl = ref("")
const newUserPermission = ref({
  permissions: []
})
const topicUsersPaginationRef = ref(null)

const handleTopicUsersFetched = (items) => {
  topicUsers.value = items.map(user => ({
    ...user,
    draft_permissions: [...user.permissions]
  }))
}

const topicUsersFields = [
  { key: 'username', label: 'Username', tdClass: 'align-middle' },
  { key: 'email', label: 'Email', tdClass: 'align-middle' },
  { key: 'permissions', label: 'Permissions', tdClass: 'align-middle' },
  { key: 'actions', label: 'Actions', tdClass: 'align-middle' },
]

const canAssignUserPermission = computed(() => {
  return assignUserId.value !== '' && newUserPermission.value.permissions.length > 0
})

const showUsersModal = (topic) => {
  selectedTopic.value = topic
  topicUsers.value = []
  newUserPermission.value = {
    permissions: []
  }
  isUsersModalVisible.value = true
  topicUsersUrl.value = `/v1/topic/${selectedTopic?.value.id}/user`
  topicUsersPaginationRef.value?.fetchData()
}

const onUsersModalHidden = () => {
  selectedTopic.value = null
  topicUsers.value = []
  topicUsersUrl.value = ''
}

function hasManagePermissionOf(permissions, permission) {
  if (!permissions) {
    return false
  }
  if (permission === TopicPermissionType["READ"] || permission === TopicPermissionType["MANAGE_READ"]) {
    return permissions.some(p => p === TopicPermissionType["MANAGE_READ"].value)
  }
  if (permission === TopicPermissionType["WRITE"] || permission === TopicPermissionType["MANAGE_WRITE"]) {
    return permissions.some(p => p === TopicPermissionType["MANAGE_WRITE"].value)
  }
  if (permission === TopicPermissionType["READ_IN_PUBLIC"] || permission === TopicPermissionType["MANAGE_READ_IN_PUBLIC"]) {
    return permissions.some(p => p === TopicPermissionType["MANAGE_READ_IN_PUBLIC"].value)
  }
  return false
}

function canChangePermission(topic, user_id, permission) {
  if (topic?.owner_id === user_id) {
      return false
  }
  return hasManagePermissionOf(topic?.permissions, permission)
}

async function saveTopicUserPermissions(user) {
  if (!selectedTopic.value) return
  try {
    await apiClient.put(`/v1/topic/${selectedTopic.value.id}/user/${user.user_id}/permission`, {
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

async function assignTopicUserPermissions() {
  if (!selectedTopic.value || !canAssignUserPermission.value) return
  try {
    await apiClient.put(`/v1/topic/${selectedTopic.value.id}/user/${assignUserId.value}/permission`, {
      assign_new_user: true,
      permissions: newUserPermission.value.permissions
    })
    $toast.success('Topic user permissions assigned successfully.')
    newUserPermission.value = {
      permissions: ['Read']
    }
    assignUserId.value = ''
    topicUsersPaginationRef.value?.fetchData()
  } catch (error) {
    console.error('Failed to assign topic user permissions:', error)
    $toast.error('Failed to assign topic user permissions: ' + (error.response?.data?.message || error.message))
  }
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
