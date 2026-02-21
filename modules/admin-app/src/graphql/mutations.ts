import { gql } from "@apollo/client";

export const SEND_OTP = gql`
  mutation SendOtp($phone: String!) {
    sendOtp(phone: $phone) {
      success
    }
  }
`;

export const VERIFY_OTP = gql`
  mutation VerifyOtp($phone: String!, $code: String!) {
    verifyOtp(phone: $phone, code: $code) {
      success
    }
  }
`;

export const LOGOUT = gql`
  mutation Logout {
    logout {
      success
    }
  }
`;

export const ADD_SOURCE = gql`
  mutation AddSource($url: String!, $reason: String) {
    addSource(url: $url, reason: $reason) {
      success
      sourceId
    }
  }
`;

export const RUN_SCOUT = gql`
  mutation RunScout($query: String!) {
    runScout(query: $query) {
      success
      message
    }
  }
`;

export const STOP_SCOUT = gql`
  mutation StopScout {
    stopScout {
      success
      message
    }
  }
`;

export const RESET_SCOUT_LOCK = gql`
  mutation ResetScoutLock($query: String!) {
    resetScoutLock(query: $query) {
      success
      message
    }
  }
`;

export const CREATE_SCOUT_TASK = gql`
  mutation CreateScoutTask(
    $location: String!
    $radiusKm: Float
    $priority: Float
  ) {
    createScoutTask(
      location: $location
      radiusKm: $radiusKm
      priority: $priority
    )
  }
`;

export const CANCEL_SCOUT_TASK = gql`
  mutation CancelScoutTask($id: String!) {
    cancelScoutTask(id: $id)
  }
`;

export const TAG_STORY = gql`
  mutation TagStory($storyId: UUID!, $tagSlug: String!) {
    tagStory(storyId: $storyId, tagSlug: $tagSlug) {
      success
    }
  }
`;

export const UNTAG_STORY = gql`
  mutation UntagStory($storyId: UUID!, $tagSlug: String!) {
    untagStory(storyId: $storyId, tagSlug: $tagSlug) {
      success
    }
  }
`;

export const MERGE_TAGS = gql`
  mutation MergeTags($sourceSlug: String!, $targetSlug: String!) {
    mergeTags(sourceSlug: $sourceSlug, targetSlug: $targetSlug) {
      success
    }
  }
`;

export const SUBMIT_SOURCE = gql`
  mutation SubmitSource($url: String!, $description: String, $region: String) {
    submitSource(url: $url, description: $description, region: $region) {
      success
      sourceId
    }
  }
`;

export const DISMISS_FINDING = gql`
  mutation DismissFinding($id: String!) {
    dismissFinding(id: $id)
  }
`;
