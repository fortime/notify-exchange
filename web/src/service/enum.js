export const TransportServiceType = Object.freeze({
  1: Object.freeze({
      'name': 'Telegram',
      'code': 'TELEGRAM',
  }),
  2: Object.freeze({
      'name': 'Http',
      'code': 'HTTP',
  }),
});

export const TopicPermissionType = Object.freeze({
  'READ': Object.freeze({
      name: 'Read',
      code: 1,
      value: 'Read',
  }),
  'WRITE': Object.freeze({
      name: 'Write',
      code: 2,
      value: 'Write',
  }),
  'READ_IN_PUBLIC': Object.freeze({
      name: 'Read in Public',
      code: 3,
      value: 'ReadInPublic',
  }),
  'MANAGE_READ': Object.freeze({
      name: 'Manage Read',
      code: 11,
      value: 'ManageRead',
  }),
  'MANAGE_WRITE': Object.freeze({
      name: 'Manage Write',
      code: 12,
      value: 'ManageWrite',
  }),
  'MANAGE_READ_IN_PUBLIC': Object.freeze({
      name: 'Manage Read in Public',
      code: 13,
      value: 'ManageReadInPublic',
  }),
});
