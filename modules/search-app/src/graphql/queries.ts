import { gql } from "@apollo/client";

export const SIGNALS_IN_BOUNDS = gql`
  query SignalsInBounds(
    $minLat: Float!
    $maxLat: Float!
    $minLng: Float!
    $maxLng: Float!
    $limit: Int
  ) {
    signalsInBounds(
      minLat: $minLat
      maxLat: $maxLat
      minLng: $minLng
      maxLng: $maxLng
      limit: $limit
    ) {
      ... on GqlGatheringSignal {
        id
        title
        summary
        confidence
        causeHeat
        channelDiversity
        extractedAt
        location { lat lng }
        locationName
        startsAt
        organizer
      }
      ... on GqlAidSignal {
        id
        title
        summary
        confidence
        causeHeat
        channelDiversity
        extractedAt
        location { lat lng }
        locationName
        availability
      }
      ... on GqlNeedSignal {
        id
        title
        summary
        confidence
        causeHeat
        channelDiversity
        extractedAt
        location { lat lng }
        locationName
        urgency
        whatNeeded
      }
      ... on GqlNoticeSignal {
        id
        title
        summary
        confidence
        causeHeat
        channelDiversity
        extractedAt
        location { lat lng }
        locationName
        severity
      }
      ... on GqlTensionSignal {
        id
        title
        summary
        confidence
        causeHeat
        channelDiversity
        extractedAt
        location { lat lng }
        locationName
        severity
        whatWouldHelp
      }
    }
  }
`;

export const STORIES_IN_BOUNDS = gql`
  query StoriesInBounds(
    $minLat: Float!
    $maxLat: Float!
    $minLng: Float!
    $maxLng: Float!
    $tag: String
    $limit: Int
  ) {
    storiesInBounds(
      minLat: $minLat
      maxLat: $maxLat
      minLng: $minLng
      maxLng: $maxLng
      tag: $tag
      limit: $limit
    ) {
      id
      headline
      summary
      signalCount
      energy
      velocity
      centroidLat
      centroidLng
      dominantType
      arc
      category
      lede
      tags {
        slug
        name
      }
    }
  }
`;

export const SEARCH_SIGNALS_IN_BOUNDS = gql`
  query SearchSignalsInBounds(
    $query: String!
    $minLat: Float!
    $maxLat: Float!
    $minLng: Float!
    $maxLng: Float!
    $limit: Int
  ) {
    searchSignalsInBounds(
      query: $query
      minLat: $minLat
      maxLat: $maxLat
      minLng: $minLng
      maxLng: $maxLng
      limit: $limit
    ) {
      score
      signal {
        ... on GqlGatheringSignal {
          id
          title
          summary
          confidence
          causeHeat
          channelDiversity
          extractedAt
          location { lat lng }
          locationName
          startsAt
          organizer
        }
        ... on GqlAidSignal {
          id
          title
          summary
          confidence
          causeHeat
          channelDiversity
          extractedAt
          location { lat lng }
          locationName
          availability
        }
        ... on GqlNeedSignal {
          id
          title
          summary
          confidence
          causeHeat
          channelDiversity
          extractedAt
          location { lat lng }
          locationName
          urgency
          whatNeeded
        }
        ... on GqlNoticeSignal {
          id
          title
          summary
          confidence
          causeHeat
          channelDiversity
          extractedAt
          location { lat lng }
          locationName
          severity
        }
        ... on GqlTensionSignal {
          id
          title
          summary
          confidence
          causeHeat
          channelDiversity
          extractedAt
          location { lat lng }
          locationName
          severity
          whatWouldHelp
        }
      }
    }
  }
`;

export const SEARCH_STORIES_IN_BOUNDS = gql`
  query SearchStoriesInBounds(
    $query: String!
    $minLat: Float!
    $maxLat: Float!
    $minLng: Float!
    $maxLng: Float!
    $limit: Int
  ) {
    searchStoriesInBounds(
      query: $query
      minLat: $minLat
      maxLat: $maxLat
      minLng: $minLng
      maxLng: $maxLng
      limit: $limit
    ) {
      score
      story {
        id
        headline
        summary
        signalCount
        energy
        velocity
        centroidLat
        centroidLng
        dominantType
        arc
        category
        lede
        tags {
          slug
          name
        }
      }
      topMatchingSignalTitle
    }
  }
`;

export const STORY_DETAIL = gql`
  query StoryDetail($id: UUID!) {
    story(id: $id) {
      id
      headline
      summary
      signalCount
      energy
      velocity
      arc
      category
      lede
      narrative
      actionGuidance
      tags {
        slug
        name
      }
    }
  }
`;

export const TAGS = gql`
  query Tags($limit: Int) {
    tags(limit: $limit) {
      slug
      name
    }
  }
`;

export const SIGNAL_DETAIL = gql`
  query SignalDetail($id: UUID!) {
    signal(id: $id) {
      ... on GqlGatheringSignal {
        id
        title
        summary
        confidence
        causeHeat
        channelDiversity
        extractedAt
        location { lat lng precision }
        locationName
        sourceUrl
        startsAt
        endsAt
        organizer
        isRecurring
        evidence { sourceUrl snippet relevance }
        story { id headline }
      }
      ... on GqlAidSignal {
        id
        title
        summary
        confidence
        causeHeat
        channelDiversity
        extractedAt
        location { lat lng precision }
        locationName
        sourceUrl
        availability
        isOngoing
        evidence { sourceUrl snippet relevance }
        story { id headline }
      }
      ... on GqlNeedSignal {
        id
        title
        summary
        confidence
        causeHeat
        channelDiversity
        extractedAt
        location { lat lng precision }
        locationName
        sourceUrl
        urgency
        whatNeeded
        goal
        evidence { sourceUrl snippet relevance }
        story { id headline }
      }
      ... on GqlNoticeSignal {
        id
        title
        summary
        confidence
        causeHeat
        channelDiversity
        extractedAt
        location { lat lng precision }
        locationName
        sourceUrl
        severity
        category
        evidence { sourceUrl snippet relevance }
        story { id headline }
      }
      ... on GqlTensionSignal {
        id
        title
        summary
        confidence
        causeHeat
        channelDiversity
        extractedAt
        location { lat lng precision }
        locationName
        sourceUrl
        severity
        category
        whatWouldHelp
        evidence { sourceUrl snippet relevance }
        story { id headline }
      }
    }
  }
`;
