export const STAGES = [
  'discovered',
  'preparing',
  'applied',
  'recruiter_screen',
  'interview',
  'technical_test',
  'final_interview',
  'offer',
  'rejected',
  'withdrawn',
] as const;
export type Stage = (typeof STAGES)[number];
export const STAGE_LABELS: Record<Stage, string> = {
  discovered: 'Discovered',
  preparing: 'Preparing',
  applied: 'Applied',
  recruiter_screen: 'Recruiter screen',
  interview: 'Interview',
  technical_test: 'Technical test',
  final_interview: 'Final interview',
  offer: 'Offer',
  rejected: 'Rejected',
  withdrawn: 'Withdrawn',
};
export interface JobApplication {
  id: string;
  company: string;
  role: string;
  location: string;
  jobUrl: string;
  source: string;
  appliedAt: string | null;
  currentStage: Stage;
  nextAction: string | null;
  nextActionDueAt: string | null;
  notes: string;
  archived: boolean;
  stageManuallySet: boolean;
  createdAt: string;
  updatedAt: string;
}
export interface ApplicationInput {
  id?: string | null;
  company: string;
  role: string;
  location: string;
  jobUrl: string;
  source: string;
  appliedAt: string | null;
  currentStage: Stage;
  nextAction: string | null;
  nextActionDueAt: string | null;
  notes: string;
  archived: boolean;
  sourceEmailId?: string | null;
}
export interface Email {
  id: string;
  gmailMessageId: string;
  gmailThreadId: string;
  gmailAccountId: string | null;
  remoteDeleted: boolean;
  senderName: string;
  senderEmail: string;
  recipients: string[];
  subject: string;
  snippet: string;
  bodyText: string;
  receivedAt: string;
  contentHash: string;
  category: string;
  classificationConfidence: number;
  classificationSource: string;
  classifiedAt: string | null;
  requiresAction: boolean;
  suggestedAction: string | null;
  createdAt: string;
  updatedAt: string;
  isJobRelated: boolean;
  reasoningCode: string;
  extractedCompany: string | null;
  extractedRole: string | null;
  suggestedStage: Stage | null;
  eventType: string | null;
  deadline: string | null;
  manualOverride: boolean;
  ignored: boolean;
  actionCompleted: boolean;
}
export interface ApplicationEmail {
  applicationId: string;
  emailId: string;
  associationConfidence: number;
  associationSource: string;
  createdAt: string;
}
export interface ApplicationEvent {
  id: string;
  applicationId: string;
  eventType: string;
  eventDate: string;
  sourceEmailId: string | null;
  confidence: number | null;
  eventSource: string;
  notes: string | null;
  createdAt: string;
}
export interface AppSettings {
  syncEmailLimit: number;
  localConfidenceAcceptThreshold: number;
  localConfidenceGeminiThreshold: number;
  aiMode: 'off' | 'uncertain' | 'candidates';
}
export interface AiSettings {
  model: string;
  maxRequestsPerRun: number;
}
export interface AiUsage {
  requests: number;
  succeeded: number;
  inputTokens: number;
  outputTokens: number;
  cacheHits: number;
}
export interface AiStatus {
  configured: boolean;
  credentialStoreAvailable: boolean;
  settings: AiSettings;
  usage: AiUsage;
  running: boolean;
  processed: number;
  total: number;
  applied: number;
  cacheHits: number;
  requests: number;
  lastError: string | null;
  lastMessage: string | null;
}
export interface WorkspaceSnapshot {
  applications: JobApplication[];
  emails: Email[];
  links: ApplicationEmail[];
  events: ApplicationEvent[];
  settings: AppSettings;
  databasePath: string;
}
export interface GmailStatus {
  configured: boolean;
  connected: boolean;
  credentialStoreAvailable: boolean;
  clientId: string | null;
  accountEmail: string | null;
  phase:
    | 'not_configured'
    | 'disconnected'
    | 'connecting'
    | 'connected'
    | 'syncing'
    | 'reconnect_required'
    | 'error';
  lastSyncedAt: string | null;
  lastError: string | null;
  processed: number;
  total: number;
  imported: number;
  skipped: number;
  failed: number;
}
export type Page = 'jobs' | 'actions' | 'emails' | 'other' | 'unmatched' | 'settings';
export type EmailDisposition = 'not_job' | 'ignored' | 'restore';
