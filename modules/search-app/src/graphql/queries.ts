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
      ... on GqlResourceSignal {
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
      ... on GqlHelpRequestSignal {
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
      ... on GqlAnnouncementSignal {
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
      ... on GqlConcernSignal {
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
        opposing
      }
      ... on GqlConditionSignal {
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
        measurement
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
        ... on GqlResourceSignal {
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
        ... on GqlHelpRequestSignal {
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
        ... on GqlAnnouncementSignal {
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
        ... on GqlConcernSignal {
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
          opposing
        }
        ... on GqlConditionSignal {
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
          measurement
        }
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
        citations { sourceUrl snippet relevance }
      }
      ... on GqlResourceSignal {
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
        citations { sourceUrl snippet relevance }
      }
      ... on GqlHelpRequestSignal {
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
        statedGoal
        citations { sourceUrl snippet relevance }
      }
      ... on GqlAnnouncementSignal {
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
        citations { sourceUrl snippet relevance }
      }
      ... on GqlConcernSignal {
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
        opposing
        citations { sourceUrl snippet relevance }
      }
      ... on GqlConditionSignal {
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
        subject
        observedBy
        measurement
        affectedScope
        citations { sourceUrl snippet relevance }
      }
    }
  }
`;

// --- Situation queries ---

export const SITUATIONS_IN_BOUNDS = gql`
  query SituationsInBounds(
    $minLat: Float!
    $maxLat: Float!
    $minLng: Float!
    $maxLng: Float!
    $arc: String
    $limit: Int
  ) {
    situationsInBounds(
      minLat: $minLat
      maxLat: $maxLat
      minLng: $minLng
      maxLng: $maxLng
      arc: $arc
      limit: $limit
    ) {
      id
      headline
      lede
      arc
      temperature
      signalCount
      centroidLat
      centroidLng
      locationName
      clarity
      category
    }
  }
`;

export const SITUATION_DETAIL = gql`
  query SituationDetail($id: UUID!) {
    situation(id: $id) {
      id
      headline
      lede
      arc
      temperature
      tensionHeat
      entityVelocity
      amplification
      responseCoverage
      clarityNeed
      clarity
      signalCount
      tensionCount
      dispatchCount
      centroidLat
      centroidLng
      locationName
      firstSeen
      lastUpdated
      sensitivity
      category
      briefingBody
      signals(limit: 50) {
        ... on GqlGatheringSignal {
          id title summary locationName startsAt endsAt actionUrl organizer isRecurring
        }
        ... on GqlResourceSignal {
          id title summary locationName actionUrl availability isOngoing
        }
        ... on GqlHelpRequestSignal {
          id title summary locationName urgency whatNeeded actionUrl statedGoal
        }
        ... on GqlAnnouncementSignal {
          id title summary locationName severity subject effectiveDate sourceAuthority
        }
        ... on GqlConcernSignal {
          id title summary locationName severity subject opposing
        }
        ... on GqlConditionSignal {
          id title summary locationName severity subject observedBy measurement affectedScope
        }
      }
      dispatches(limit: 50) {
        id
        body
        signalIds
        createdAt
        dispatchType
        supersedes
        flaggedForReview
        flagReason
        fidelityScore
      }
    }
  }
`;

export const SITUATIONS = gql`
  query Situations($limit: Int) {
    situations(limit: $limit) {
      id
      headline
      lede
      arc
      temperature
      signalCount
      centroidLat
      centroidLng
      locationName
      clarity
    }
  }
`;
